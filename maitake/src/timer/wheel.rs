use super::*;
use cordyceps::List;

pub(super) struct Wheel {
    level: usize,
    slots: [List<sleep::Entry>; Self::LENGTH],
}

impl Wheel {
    pub(super) const LENGTH: usize = 64;
    pub(super) const fn new(level: usize) -> Self {
        const NEW_LIST: List<sleep::Entry> = List::new();
        Self {
            level,
            slots: [NEW_LIST; Self::LENGTH],
        }
    }
}
