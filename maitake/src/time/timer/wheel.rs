use super::{sleep, Ticks};
use cordyceps::List;
use core::{pin::Pin, ptr};
use mycelium_util::fmt;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(in crate::time) struct Core {
    /// The total number of ticks that have now since this timer started.
    now: Ticks,

    /// The actual timer wheels.
    wheels: [Wheel; Self::WHEELS],
}

/// _The Wheel of Time_, by Robert Jordan
struct Wheel {
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

    slots: [List<sleep::Entry>; Self::SLOTS],
}

#[derive(Debug)]
struct Deadline {
    ticks: Ticks,
    slot: usize,
    wheel: usize,
}

// === impl Core ===

impl Core {
    const WHEELS: usize = Wheel::BITS;

    pub(super) const MAX_SLEEP_TICKS: u64 = (1 << (Wheel::BITS * Self::WHEELS)) - 1;

    pub(super) const fn new() -> Self {
        // Initialize the wheels.
        // XXX(eliza): we would have to do this extremely gross thing if we
        // wanted to support a variable number of wheels, because const fn...
        /*
        // Used as an initializer when constructing a new `Core`.
        const NEW_WHEEL: Wheel = Wheel::empty();

        let mut wheels = [NEW_WHEEL; Self::WHEELS];n
        let mut level = 0;
        while level < Self::WHEELS {
            wheels[level].level = level;
            wheels[level].ticks_per_slot = wheel::ticks_per_slot(level);
            level += 1;
        }
        */

        Self {
            now: 0,
            wheels: [
                Wheel::new(0),
                Wheel::new(1),
                Wheel::new(2),
                Wheel::new(3),
                Wheel::new(4),
                Wheel::new(5),
            ],
        }
    }

    #[inline(never)]
    pub(super) fn advance(&mut self, ticks: Ticks) -> usize {
        let now = self.now + ticks;
        let mut fired = 0;
        // sleeps that need to be rescheduled on lower-level wheels need to be
        // processed after we have finished turning the wheel, to avoid looping
        // infinitely.

        let mut pending_reschedule = List::<sleep::Entry>::new();
        while let Some(deadline) = self.next_deadline() {
            if deadline.ticks > now {
                break;
            }

            let mut fired_this_turn = 0;
            let entries = self.wheels[deadline.wheel].take(deadline.slot);
            debug!(
                now = self.now,
                deadline.ticks,
                entries = entries.len(),
                "turning wheel to"
            );

            for entry in entries {
                let entry_deadline = unsafe { entry.as_ref() }.deadline(self);

                if test_dbg!(entry_deadline) > test_dbg!(now) {
                    // this timer was on the top-level wheel and needs to be
                    // rescheduled on a lower-level wheel, rather than firing now.
                    debug_assert_ne!(
                        deadline.wheel, 0,
                        "if a timer is being rescheduled, it must not have been on the lowest-level wheel"
                    );
                    // this timer will need to be rescheduled.
                    pending_reschedule.push_front(entry);
                } else {
                    // otherwise, fire the timer.
                    unsafe {
                        fired_this_turn += 1;
                        entry.as_ref().fire();
                    }
                }
            }

            trace!(at = self.now, firing = fired_this_turn, "firing timers");

            self.now = deadline.ticks;
            fired += fired_this_turn;
        }

        self.now = now;

        // reschedule pending sleeps.
        debug!(
            now = self.now,
            fired,
            rescheduled = pending_reschedule.len(),
            "wheel turned to"
        );
        for entry in pending_reschedule {
            let deadline = unsafe { entry.as_ref() }.deadline(self);
            debug_assert_ne!(deadline, 0);
            self.insert_sleep_at(deadline, entry)
        }

        fired
    }

    pub(super) fn cancel_sleep(&mut self, sleep: Pin<&mut sleep::Entry>) {
        let deadline = sleep.deadline(self);
        trace!(
            sleep.addr = ?format_args!("{:p}", sleep),
            sleep.ticks = *sleep.as_ref().project_ref().ticks,
            sleep.deadline = deadline,
            now = self.now,
            "canceling sleep"
        );
        let wheel = self.wheel_index(deadline);
        self.wheels[wheel].remove(deadline, sleep);
    }

    pub(super) fn register_sleep(&mut self, mut sleep: ptr::NonNull<sleep::Entry>) {
        let deadline = {
            let entry = unsafe { sleep.as_mut() };
            let deadline = entry.ticks + self.now;
            // set the entry's deadline with the wheel's current time.
            entry.set_deadline(self, deadline);

            trace!(
                sleep.addr = ?sleep,
                sleep.ticks = entry.ticks,
                sleep.start_time = self.now,
                sleep.deadline = deadline,
                "registering sleep"
            );

            deadline
        };

        self.insert_sleep_at(deadline, sleep)
    }

    fn insert_sleep_at(&mut self, deadline: Ticks, sleep: ptr::NonNull<sleep::Entry>) {
        let wheel = self.wheel_index(deadline);
        trace!(wheel, sleep.deadline = deadline, sleep.addr = ?sleep, "inserting sleep");
        self.wheels[wheel].insert(deadline, sleep);
    }

    /// Returns the deadline and location of the next firing timer in the wheel.
    #[inline]
    fn next_deadline(&self) -> Option<Deadline> {
        self.wheels.iter().find_map(|wheel| {
            let next_deadline = wheel.next_deadline(self.now)?;
            trace!(
                now = self.now,
                next_deadline.ticks,
                next_deadline.wheel,
                next_deadline.slot,
            );
            Some(next_deadline)
        })
    }

    #[inline]
    fn wheel_index(&self, ticks: Ticks) -> usize {
        wheel_index(self.now, ticks)
    }
}

fn wheel_index(now: Ticks, ticks: Ticks) -> usize {
    const WHEEL_MASK: u64 = (1 << Wheel::BITS) - 1;

    // mask out the bits representing the index in the wheel
    let mut wheel_indices = now ^ ticks | WHEEL_MASK;

    // put sleeps over the max duration in the top level wheel
    if wheel_indices >= Core::MAX_SLEEP_TICKS {
        wheel_indices = Core::MAX_SLEEP_TICKS - 1;
    }

    let zeros = wheel_indices.leading_zeros();
    let rest = u64::BITS - 1 - zeros;

    rest as usize / Core::WHEELS
}

impl Wheel {
    /// The number of slots per timer wheel is fixed at 64 slots.
    ///
    /// This is because we can use a 64-bit bitmap for each wheel to store which
    /// slots are occupied.
    const SLOTS: usize = 64;
    const BITS: usize = Self::SLOTS.trailing_zeros() as usize;
    const fn new(level: usize) -> Self {
        // linked list const initializer
        const NEW_LIST: List<sleep::Entry> = List::new();

        // how many ticks does a single slot represent in a wheel of this level?
        let ticks_per_slot = Self::SLOTS.pow(level as u32) as Ticks;
        let ticks_per_wheel = ticks_per_slot * Self::SLOTS as u64;

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
            slots: [NEW_LIST; Self::SLOTS],
        }
    }

    /// Insert a sleep entry into this wheel.
    fn insert(&mut self, deadline: Ticks, sleep: ptr::NonNull<sleep::Entry>) {
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
    fn remove(&mut self, deadline: Ticks, sleep: Pin<&mut sleep::Entry>) {
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

    fn take(&mut self, slot: usize) -> List<sleep::Entry> {
        debug_assert!(
            self.occupied_slots & (1 << slot) != 0,
            "taking an unoccupied slot!"
        );
        let list = self.slots[slot].split_off(0);
        debug_assert!(
            !list.is_empty(),
            "if a slot is occupied, its list must not be empty"
        );
        self.clear_slot(slot);
        list
    }

    fn next_deadline(&self, now: u64) -> Option<Deadline> {
        let distance = self.next_slot_distance(now)?;

        let slot = distance % Self::SLOTS;
        // does the next slot wrap this wheel around from the now slot?
        let skipped = distance.saturating_sub(Self::SLOTS);

        debug_assert!(distance < Self::SLOTS * 2);
        debug_assert!(
            skipped == 0 || self.level == Core::WHEELS - 1,
            "if the next expiring slot wraps around, we must be on the top level wheel\
            \n    dist: {distance}\
            \n    slot: {slot}\
            \n skipped: {skipped}\
            \n   level: {}",
            self.level,
        );

        // when did the current rotation of this wheel begin? since all wheels
        // represent a power-of-two number of ticks, we can determine the
        // beginning of this rotation by masking out the bits for all lower wheels.
        let rotation_start = now & self.wheel_mask;
        // the next deadline is the start of the current rotation, plus the next
        // slot's value.
        let ticks = {
            let skipped_ticks = skipped as u64 * self.ticks_per_wheel;
            rotation_start + (slot as u64 * self.ticks_per_slot) + skipped_ticks
        };

        test_trace!(
            now,
            wheel = self.level,
            rotation_start,
            slot,
            skipped,
            ticks,
            "Wheel::next_deadline"
        );

        let deadline = Deadline {
            ticks,
            slot,
            wheel: self.level,
        };

        Some(deadline)
    }

    /// Returns the slot index of the next firing timer.
    fn next_slot_distance(&self, now: Ticks) -> Option<usize> {
        if self.occupied_slots == 0 {
            return None;
        }

        // which slot is indexed by the `now` timestamp?
        let now_slot = (now / self.ticks_per_slot) as u32 % Self::SLOTS as u32;
        let next_dist = next_set_bit(self.occupied_slots, now_slot)?;

        test_trace!(
            now_slot,
            next_dist,
            occupied = ?fmt::bin(self.occupied_slots),
            "next_slot_distance"
        );
        Some(next_dist)
    }

    fn clear_slot(&mut self, slot_index: usize) {
        debug_assert!(slot_index < Self::SLOTS);
        self.occupied_slots &= !(1 << slot_index);
    }

    fn fill_slot(&mut self, slot_index: usize) {
        debug_assert!(slot_index < Self::SLOTS);
        self.occupied_slots |= 1 << slot_index;
    }

    /// Given a duration, returns the slot into which an entry for that duratio
    /// would be inserted.
    const fn slot_index(&self, ticks: Ticks) -> usize {
        let shift = self.level * Self::BITS;
        ((ticks >> shift) % Self::SLOTS as u64) as usize
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
    // XXX(eliza): there's probably a way to implement this with less
    // branches via some kind of bit magic...
    debug_assert!(offset < 64, "offset: {offset}");
    if bitmap == 0 {
        return None;
    }
    let shifted = bitmap >> offset;
    let zeros = if shifted == 0 {
        bitmap.rotate_right(offset).trailing_zeros()
    } else {
        shifted.trailing_zeros()
    };
    Some(zeros as usize + offset as usize)
}
