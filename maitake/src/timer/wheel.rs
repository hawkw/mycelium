use super::*;
use cordyceps::List;
use core::ptr;
use mycelium_util::fmt;

pub(super) struct Wheel {
    /// A bitmap of the slots that are occupied.
    ///
    /// See <https://lwn.net/Articles/646056/> for details on
    /// this strategy.
    occupied: u64,

    slots: [List<sleep::Entry>; SLOTS],
}

/// The number of slots per timer wheel is fixed at 64 slots.
///
/// This is because we can use a 64-bit bitmap for each wheel to store which
/// slots are occupied.
pub(super) const SLOTS: usize = 64;
pub(super) const BITS: usize = SLOTS.trailing_zeros() as usize;

impl Wheel {
    pub(super) const fn new() -> Self {
        const NEW_LIST: List<sleep::Entry> = List::new();
        Self {
            occupied: 0,
            slots: [NEW_LIST; SLOTS],
        }
    }

    /// Insert a sleep entry into this wheel.
    pub(super) fn insert(&mut self, wheel: usize, ticks: Ticks, sleep: ptr::NonNull<sleep::Entry>) {
        let slot = Self::slot_index(wheel, ticks);
        trace!(
            wheel,
            sleep = ?fmt::ptr(sleep),
            sleep.ticks = ticks,
            sleep.slot = slot,
            "Wheel::insert",
        );

        // insert the sleep entry into the appropriate linked list.
        self.slots[slot].push_front(sleep);
        // toggle the occupied bit for that slot.
        self.occupied |= 1 << slot;
    }

    /// Remove a sleep entry from this wheel.
    pub(super) fn remove(&mut self, wheel: usize, ticks: Ticks, sleep: Pin<&mut sleep::Entry>) {
        let slot = Self::slot_index(wheel, ticks);
        let _removed = unsafe {
            // safety: we will not use the `NonNull` to violate pinning
            // invariants; it's used only to insert the sleep into the intrusive
            // list. It's safe to remove the sleep from the linked list because
            // we know it's in this list (provided the rest of the timer wheel
            // is like...working...)
            let ptr = ptr::NonNull::from(Pin::into_inner_unchecked(sleep));
            trace!(
                wheel,
                sleep = ?fmt::ptr(ptr),
                sleep.ticks = ticks,
                sleep.slot = slot,
                "Wheel::remove",
            );

            self.slots[slot].remove(ptr).is_some()
        };

        debug_assert!(_removed);

        if self.slots[slot].is_empty() {
            // if that was the only sleep in that slot's linked list, clear the
            // corresponding occupied bit.
            self.occupied &= !(1 << slot);
        }
    }

    /// Given this wheel's level and a timestamp, returns the slot into which
    /// that timestamp would be inserted.
    const fn slot_index(level: usize, ticks: Ticks) -> usize {
        let shift = level * BITS;
        ((ticks >> shift) % SLOTS as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitshift_is_correct() {
        assert_eq!(1 << BITS, SLOTS);
    }

    #[test]
    fn slot_indices() {
        for i in 0..64 {
            let slot_index = Wheel::slot_index(0, i);
            assert_eq!(i as usize, slot_index, "Wheel::slot_index(0, {i}) == {i}")
        }

        for wheel in 1..Core::WHEELS {
            for i in wheel..SLOTS {
                let ticks = i * usize::pow(SLOTS, wheel as u32);
                let slot_index = Wheel::slot_index(wheel, ticks as u64);
                assert_eq!(
                    i as usize, slot_index,
                    "Wheel::slot_index({wheel}, {ticks}) == {i}"
                )
            }
        }
    }
}
