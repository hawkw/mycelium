use crate::loom::sync::atomic::{
    self, AtomicUsize,
    Ordering::{self, *},
};
use core::fmt;
use mycelium_util::bits::PackUsize;

#[derive(Clone, Copy)]
pub(crate) struct State(usize);

#[repr(transparent)]
pub(super) struct StateVar(AtomicUsize);

impl State {
    const RUNNING: PackUsize = PackUsize::least_significant(1);
    const NOTIFIED: PackUsize = Self::RUNNING.next(1);
    const COMPLETED: PackUsize = Self::NOTIFIED.next(1);
    const REFS: PackUsize = Self::COMPLETED.remaining();

    const REF_ONE: usize = Self::REFS.first_bit();
    const REF_MAX: usize = Self::REFS.raw_mask();
    // const STATE_MASK: usize =
    //     Self::RUNNING.raw_mask() | Self::NOTIFIED.raw_mask() | Self::COMPLETED.raw_mask();

    #[inline]
    pub(crate) fn is_running(self) -> bool {
        Self::RUNNING.contained_in_all(self.0)
    }

    #[inline]
    pub(crate) fn is_notified(self) -> bool {
        Self::NOTIFIED.contained_in_all(self.0)
    }

    #[inline]
    pub(crate) fn is_completed(self) -> bool {
        Self::NOTIFIED.contained_in_all(self.0)
    }

    #[inline]
    pub(crate) fn ref_count(self) -> usize {
        Self::REFS.unpack(self.0)
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("running", &self.is_running())
            .field("notified", &self.is_notified())
            .field("completed", &self.is_completed())
            .field("ref_count", &self.ref_count())
            .field("bits", &format_args!("{:#b}", self.0))
            .finish()
    }
}

impl fmt::Binary for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "State({:#b})", self.0)
    }
}

// === impl StateVar ===

impl StateVar {
    pub fn new() -> Self {
        Self(AtomicUsize::new(State::REF_ONE))
    }

    #[inline]
    pub(super) fn clone_ref(&self) {
        // Using a relaxed ordering is alright here, as knowledge of the
        // original reference prevents other threads from erroneously deleting
        // the object.
        //
        // As explained in the [Boost documentation][1], Increasing the
        // reference counter can always be done with memory_order_relaxed: New
        // references to an object can only be formed from an existing
        // reference, and passing an existing reference from one thread to
        // another must already provide any required synchronization.
        //
        // [1]: (www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html)
        let old_refs = test_dbg!(self.0.fetch_add(State::REF_ONE, Relaxed));

        // However we need to guard against massive refcounts in case someone
        // is `mem::forget`ing tasks. If we don't do this the count can overflow
        // and users will use-after free. We racily saturate to `isize::MAX` on
        // the assumption that there aren't ~2 billion threads incrementing
        // the reference count at once. This branch will never be taken in
        // any realistic program.
        //
        // We abort because such a program is incredibly degenerate, and we
        // don't care to support it.
        if test_dbg!(old_refs > State::REF_MAX) {
            panic!("task reference count overflow");
        }
    }

    #[inline]
    pub(super) fn drop_ref(&self) -> bool {
        // Because `cores` is already atomic, we do not need to synchronize
        // with other threads unless we are going to delete the task.
        let old_refs = test_dbg!(self.0.fetch_sub(State::REF_ONE, Release));

        // Did we drop the last ref?
        if test_dbg!(old_refs != State::REF_ONE) {
            return false;
        }

        atomic::fence(Acquire);
        true
    }

    pub(super) fn load(&self, order: Ordering) -> State {
        State(self.0.load(order))
    }
}

impl fmt::Debug for StateVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.load(Relaxed).fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_specs_valid() {
        PackUsize::assert_all_valid(&[
            ("RUNNING", State::RUNNING),
            ("NOTIFIED", State::NOTIFIED),
            ("COMPLETED", State::COMPLETED),
            ("REFS", State::REFS),
        ])
    }
}
