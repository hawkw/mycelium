use crate::loom::sync::atomic::{
    self, AtomicUsize,
    Ordering::{self, *},
};
use mycelium_util::bits::PackUsize;
#[derive(Clone, Copy)]
pub(crate) struct State(usize);
#[derive(Default)]
#[repr(transparent)]
pub(super) struct StateVar(AtomicUsize);

impl State {
    const RUNNING: PackUsize = PackUsize::least_significant(1);
    const NOTIFIED: PackUsize = Self::RUNNING.next(1);
    const COMPLETED: PackUsize = Self::NOTIFIED.next(1);
    const REFS: PackUsize = Self::COMPLETED.remaining();

    const REF_ONE: usize = Self::REFS.first_bit();
    const STATE_MASK: usize =
        Self::RUNNING.raw_mask() | Self::NOTIFIED.raw_mask() | Self::COMPLETED.raw_mask();
}

impl StateVar {
    pub(super) fn clone_ref(&self) {
        self.0.fetch_add(State::REF_ONE, Relaxed);
    }

    pub(super) fn drop_ref(&self) -> bool {
        let val = self.0.fetch_sub(State::REF_ONE, Relaxed);
        if State::REFS.unpack(val) == 1 {
            // Did we drop the last ref?
            atomic::fence(Release);
            return true;
        }
        false
    }

    pub(super) fn load(&self, order: Ordering) -> State {
        State(self.0.load(order))
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
