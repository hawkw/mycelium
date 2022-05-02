use crate::loom::sync::atomic::{AtomicUsize, Ordering};
use mycelium_util::bits::{self, PackUsize};

#[repr(transparent)]
pub(crate) struct StateVar(AtomicUsize);
pub(crate) struct State(usize);

impl State {
    const RUNNING: PackUsize = PackUsize::least_significant(1);
    const NOTIFIED: PackUsize = Self::RUNNING.next(1);
    const COMPLETED: PackUsize = Self::NOTIFIED.next(1);
    const REFS: PackUsize = Self::COMPLETED.remaining();

    const REF_ONE: usize = Self::REFS.first_bit();
    const STATE_MASK: usize =
        Self::RUNNING.raw_mask() | Self::NOTIFIED.raw_mask() | Self::COMPLETED.raw_mask();
}

impl StateVar {}

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
