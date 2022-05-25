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
    // const STATE_MASK: usize =
    //     RUNNING.raw_mask() | NOTIFIED.raw_mask() | COMPLETED.raw_mask();

    #[inline]
    pub(crate) fn ref_count(self) -> usize {
        REFS.unpack(self.0)
    }

    #[inline]
    pub(crate) fn is(self, field: PackUsize) -> bool {
        field.contained_in_all(self.0)
    }

    #[inline]
    pub(crate) fn set(self, field: PackUsize, value: bool) -> Self {
        let value = if value { 1 } else { 0 };
        Self(field.pack(value, self.0))
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct FlagSet(State);
        impl FlagSet {
            const POLLING: PackUsize = POLLING;
            const WOKEN: PackUsize = WOKEN;
            const COMPLETED: PackUsize = COMPLETED;

            #[inline]
            pub(crate) fn is(&self, field: PackUsize) -> bool {
                self.0.is(field)
            }
        }

        impl fmt::Debug for FlagSet {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut _has_states = false;
                fmt_bits!(self, f, _has_states, POLLING, WOKEN, COMPLETED);
                Ok(())
            }
        }
        f.debug_struct("State")
            .field("state", &FlagSet(*self))
            .field("ref_count", &self.ref_count())
            .finish()
    }
}

impl fmt::Binary for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "State({:#b})", self.0)
    }
}

const POLLING: PackUsize = PackUsize::least_significant(1);
const WOKEN: PackUsize = POLLING.next(1);
const COMPLETED: PackUsize = WOKEN.next(1);
const REFS: PackUsize = COMPLETED.remaining();

const REF_ONE: usize = REFS.first_bit();
const REF_MAX: usize = REFS.raw_mask();
// === impl StateVar ===

impl StateVar {
    pub fn new() -> Self {
        Self(AtomicUsize::new(REF_ONE))
    }

    pub(super) fn start_poll(&self) -> Result<(), State> {
        self.transition(|state| {
            // Cannot start polling a task which is being polled on another
            // thread.
            if test_dbg!(state.is(POLLING)) {
                return Err(state);
            }

            // Cannot start polling a completed task.
            if test_dbg!(state.is(COMPLETED)) {
                return Err(state);
            }

            let new_state = state
                // The task is now being polled.
                .set(POLLING, true)
                // If the task was woken, consume the wakeup.
                .set(WOKEN, false);
            Ok(test_dbg!(new_state))
        })
    }

    pub(super) fn end_poll(&self, completed: bool) -> Result<(), State> {
        self.transition(|state| {
            // Cannot end a poll if a task is not being polled!
            debug_assert!(state.is(POLLING));
            debug_assert!(!state.is(COMPLETED));

            let new_state = state.set(POLLING, false).set(COMPLETED, completed);
            Ok(test_dbg!(new_state))
        })
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
        let old_refs = test_dbg!(self.0.fetch_add(REF_ONE, Relaxed));

        // However we need to guard against massive refcounts in case someone
        // is `mem::forget`ing tasks. If we don't do this the count can overflow
        // and users will use-after free. We racily saturate to `isize::MAX` on
        // the assumption that there aren't ~2 billion threads incrementing
        // the reference count at once. This branch will never be taken in
        // any realistic program.
        //
        // We abort because such a program is incredibly degenerate, and we
        // don't care to support it.
        if test_dbg!(old_refs > REF_MAX) {
            panic!("task reference count overflow");
        }
    }

    #[inline]
    pub(super) fn drop_ref(&self) -> bool {
        // Because `cores` is already atomic, we do not need to synchronize
        // with other threads unless we are going to delete the task.
        let old_refs = test_dbg!(self.0.fetch_sub(REF_ONE, Release));

        // Did we drop the last ref?
        if test_dbg!(old_refs != REF_ONE) {
            return false;
        }

        atomic::fence(Acquire);
        true
    }

    pub(super) fn load(&self, order: Ordering) -> State {
        State(self.0.load(order))
    }

    /// Attempts to advance this task's state by running the provided fallible
    /// `transition` function on the current [`State`].
    ///
    /// The `transition` function should return an error if the desired state
    /// transition is not possible from the task's current state, or return `Ok`
    /// with a new [`State`] if the transition is possible.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the task was successfully transitioned.
    /// - `Err(E)` with the error returned by the transition function if the
    ///   state transition is not possible.
    fn transition<E>(
        &self,
        mut transition: impl FnMut(State) -> Result<State, E>,
    ) -> Result<(), E> {
        let mut current = self.load(Acquire);
        loop {
            // Try to run the transition function to transition from `current`
            // to the next state. If the transition functiion fails (indicating
            // that the requested transition is no longer reachable from the
            // current state), bail.
            let State(next) = transition(current)?;

            match self
                .0
                .compare_exchange_weak(current.0, next, AcqRel, Acquire)
            {
                Ok(_) => return Ok(()),
                Err(actual) => current = State(actual),
            }
        }
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
            ("POLLING", POLLING),
            ("WOKEN", WOKEN),
            ("COMPLETED", COMPLETED),
            ("REFS", REFS),
        ])
    }
}
