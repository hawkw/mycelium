use super::*;
use cordyceps::List;
use core::ptr;
use mycelium_util::fmt;

/// _The Wheel of Time_, by Robert Jordan
pub(super) struct Wheel {
    /// A bitmap of the slots that are occupied.
    ///
    /// See <https://lwn.net/Articles/646056/> for details on
    /// this strategy.
    occupied_slots: u64,

    /// This wheel's level.
    level: usize,

    /// The number of ticks represented by a single slot in this wheel.
    ticks_per_slot: Ticks,

    /// The number of ticks represented by this entire wheel.
    ticks_per_wheel: Ticks,

    /// A bitmask for masking out all lower wheels' indices from a `now` timestamp.
    wheel_mask: u64,

    slots: [List<sleep::Entry>; SLOTS],
}

#[derive(Debug)]
pub(super) struct Deadline {
    pub(super) ticks: Ticks,
    pub(super) slot: usize,
    pub(super) wheel: usize,
}

/// The number of slots per timer wheel is fixed at 64 slots.
///
/// This is because we can use a 64-bit bitmap for each wheel to store which
/// slots are occupied.
pub(super) const SLOTS: usize = 64;
pub(super) const BITS: usize = SLOTS.trailing_zeros() as usize;

impl Wheel {
    pub(super) const fn new(level: usize) -> Self {
        // linked list const initializer
        const NEW_LIST: List<sleep::Entry> = List::new();

        // how many ticks does a single slot represent in a wheel of this level?
        let ticks_per_slot = SLOTS.pow(level as u32) as Ticks;
        let ticks_per_wheel = ticks_per_slot * SLOTS as u64;

        debug_assert!(ticks_per_slot.is_power_of_two());
        debug_assert!(ticks_per_wheel.is_power_of_two());

        // because `ticks_per_wheel` is a power of two, we can calculate a
        // bitmask for masking out the indices in all lower wheels from a `now`
        // timestamp.
        let wheel_mask = !(ticks_per_wheel - 1);
 
        Self {
            level,
            ticks_per_slot,
            ticks_per_wheel,
            wheel_mask,
            occupied_slots: 0,
            slots: [NEW_LIST; SLOTS],
        }
    }

    /// Insert a sleep entry into this wheel.
    pub(super) fn insert(&mut self, deadline: Ticks, sleep: ptr::NonNull<sleep::Entry>) {
        let slot = self.slot_index(deadline);
        trace!(
            wheel = self.level,
            sleep.addr = ?fmt::ptr(sleep),
            sleep.deadline = deadline,
            sleep.slot = slot,
            "Wheel::insert",
        );

        // insert the sleep entry into the appropriate linked list.
        self.slots[slot].push_front(sleep);
        // toggle the occupied bit for that slot.
        self.fill_slot(slot);
    }

    /// Remove a sleep entry from this wheel.
    pub(super) fn remove(&mut self, deadline: Ticks, sleep: Pin<&mut sleep::Entry>) {
        let slot = self.slot_index(deadline);
        let _removed = unsafe {
            // safety: we will not use the `NonNull` to violate pinning
            // invariants; it's used only to insert the sleep into the intrusive
            // list. It's safe to remove the sleep from the linked list because
            // we know it's in this list (provided the rest of the timer wheel
            // is like...working...)
            let ptr = ptr::NonNull::from(Pin::into_inner_unchecked(sleep));
            trace!(
                wheel = self.level,
                sleep.addr = ?fmt::ptr(ptr),
                sleep.deadline = deadline,
                sleep.slot = slot,
                "Wheel::remove",
            );

            self.slots[slot].remove(ptr).is_some()
        };

        debug_assert!(_removed);

        if self.slots[slot].is_empty() {
            // if that was the only sleep in that slot's linked list, clear the
            // corresponding occupied bit.
            self.clear_slot(slot);
        }
    }

    pub(super) fn take(&mut self, slot: usize) -> List<sleep::Entry> {
        debug_assert!(
            self.occupied_slots & (1 << slot) != 0,
            "taking an unoccupied slot!"
        );
        let list = self.slots[slot].split_off(0);
        debug_assert!(
            list.len() > 0,
            "if a slot is occupied, its list must not be empty"
        );
        self.clear_slot(slot);
        list
    }

    pub(super) fn next_deadline(&self, now: u64) -> Option<Deadline> {
        let slot = self.next_slot(now)?;

        // when did the current rotation of this wheel begin? since all wheels
        // represent a power-of-two number of ticks, we can determine the
        // beginning of this rotation by masking out the bits for all lower wheels.
        let rotation_start = now & self.wheel_mask;
        // the next deadline is the start of the current rotation, plus the next
        // slot's value.
        let ticks = rotation_start + (slot as u64 * self.ticks_per_slot);
        let deadline = Deadline {
            ticks,
            slot,
            wheel: self.level,
        };

        Some(deadline)
    }

    /// Returns the slot index of the next firing timer.
    pub(super) fn next_slot(&self, now: Ticks) -> Option<usize> {
        if self.occupied_slots == 0 {
            return None;
        }

        // which slot is indexed by the `now` timestamp?
        let now_slot = (now / self.ticks_per_slot) as u32 % SLOTS as u32;
        next_set_bit(self.occupied_slots, now_slot)
    }

    fn clear_slot(&mut self, slot_index: usize) {
        debug_assert!(slot_index < SLOTS);
        self.occupied_slots &= !(1 << slot_index);
    }

    fn fill_slot(&mut self, slot_index: usize) {
        debug_assert!(slot_index < SLOTS);
        self.occupied_slots |= 1 << slot_index;
    }

    /// Given a duration, returns the slot into which an entry for that duratio
    /// would be inserted.
    const fn slot_index(&self, ticks: Ticks) -> usize {
        let shift = self.level * BITS;
        ((ticks >> shift) % SLOTS as u64) as usize
    }
}

impl fmt::Debug for Wheel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wheel")
            .field("level", &self.level)
            .field("ticks_per_slot", &self.ticks_per_slot)
            .field("ticks_per_wheel", &self.ticks_per_wheel)
            .field("wheel_mask", &fmt::bin(self.wheel_mask))
            .field("occupied_slots", &fmt::bin(&self.occupied_slots))
            .field("slots", &format_args!("[...]"))
            .finish()
    }
}

/// Finds the index of the next set bit in `bitmap` after the `offset`th` bit.
/// If the `offset`th bit is set, returns `offset`.
///
/// Based on
/// <https://github.com/torvalds/linux/blob/d0e60d46bc03252b8d4ffaaaa0b371970ac16cda/include/linux/find.h#L21-L45>
fn next_set_bit(bitmap: u64, offset: u32) -> Option<usize> {
    debug_assert!(offset < 64, "offset: {offset}");
    let shifted = bitmap >> offset;
    if shifted == 0 {
        return None;
    }
    Some(shifted.trailing_zeros() as usize + offset as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn bitshift_is_correct() {
        assert_eq!(1 << BITS, SLOTS);
    }

    #[test]
    fn slot_indices() {
        let wheel = Wheel::new(0);
        for i in 0..64 {
            let slot_index = wheel.slot_index(i);
            assert_eq!(i as usize, slot_index, "wheels[0].slot_index({i}) == {i}")
        }

        for level in 1..Core::WHEELS {
            let wheel = Wheel::new(level);
            for i in level..SLOTS {
                let ticks = i * usize::pow(SLOTS, level as u32);
                let slot_index = wheel.slot_index(ticks as u64);
                assert_eq!(
                    i as usize, slot_index,
                    "wheels[{level}].slot_index({ticks}) == {i}"
                )
            }
        }
    }

    #[test]
    fn test_next_set_bit() {
        assert_eq!(dbg!(next_set_bit(0b0000_1001, 2)), Some(3));
        assert_eq!(dbg!(next_set_bit(0b0000_1001, 3)), Some(3));
        assert_eq!(dbg!(next_set_bit(0b0000_1001, 0)), Some(0));
        assert_eq!(dbg!(next_set_bit(0b0000_1001, 4)), None);
        assert_eq!(dbg!(next_set_bit(0b0000_0000, 0)), None);
        assert_eq!(dbg!(next_set_bit(0b0000_1000, 3)), Some(3));
        assert_eq!(dbg!(next_set_bit(0b0000_1000, 2)), Some(3));
        assert_eq!(dbg!(next_set_bit(0b0000_1000, 4)), None);
    }

    proptest! {
        #[test]
        fn next_set_bit_works(bitmap: u64, offset in 0..64u32) {
            // find the next set bit the slow way.
            let expected = (offset..64).find(|i| bitmap & (1 << i) != 0).map(|idx| idx as usize);
            prop_assert_eq!(next_set_bit(bitmap, offset), expected);
        }
    }
}
