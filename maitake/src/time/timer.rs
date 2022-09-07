use super::*;
use crate::loom::sync::{
    atomic::{AtomicUsize, Ordering::*},
    spin::{Mutex, MutexGuard},
};
use cordyceps::List;
use core::{pin::Pin, ptr};
use mycelium_util::fmt;

#[cfg(all(test, loom))]
mod loom;
#[cfg(all(test, not(loom)))]
mod tests;

pub type Ticks = u64;
pub struct Timer {
    /// A count of how many timer ticks have elapsed since the last time the
    /// timer's [`Core`] was updated.
    ///
    /// The timer's [`advance`] method may be called in an interrupt handler, so
    /// it cannot spin to lock the `Core` if it is busy. Instead, it tries to
    /// acquire the [`Core`] lock, and if it can't, it increments
    /// `pending_ticks`. The count of pending ticks is then consumed the next
    /// time the timer interrupt is able to lock the [`Core`].
    ///
    /// This strategy may result in some additional noise in when exactly a
    /// sleep will fire, but it allows us to avoid potential deadlocks when the
    /// timer is advanced from an interrupt handler.
    pending_ticks: AtomicUsize,
    core: Mutex<Core>,
}

pub(super) struct Core {
    /// The total number of ticks that have elapsed since this timer started.
    elapsed: Ticks,

    /// The actual timer wheels.
    wheels: [Wheel; Self::WHEELS],
}

impl Timer {
    loom_const_fn! {
        pub fn new() -> Self {
            Self {
                pending_ticks: AtomicUsize::new(0),
                core: Mutex::new(Core::new()),
            }
        }
    }

    /// The maximum number of timer ticks for [`Sleep`]s supported by this timer.
    const MAX_SLEEP_TICKS: u64 = (1 << (Wheel::BITS * Core::WHEELS)) - 1;

    /// Returns a future that will complete in `ticks` timer ticks.
    #[track_caller]
    pub fn sleep(&self, ticks: Ticks) -> Sleep<'_> {
        assert!(
            ticks <= Self::MAX_SLEEP_TICKS,
            "cannot sleep for more than {} ticks",
            Self::MAX_SLEEP_TICKS
        );
        Sleep::new(&self.core, ticks)
    }

    /// Add pending ticks to the timer *without* turning the wheel.
    ///
    /// This function will *never* acquire a lock, and will *never* notify any
    /// waiting [`Sleep`] futures. It can be called in an interrupt handler that
    /// cannot perform significant amounts of work.
    ///
    /// However, if this method is used, then [`Timer::force_advance`] must be
    /// called frequently from outside of the interrupt handler.
    #[inline(always)]
    pub fn pend_ticks(&self, ticks: Ticks) {
        debug_assert!(
            ticks < usize::MAX as u64,
            "cannot pend more than `usize::MAX` ticks at once!"
        );
        self.pending_ticks.fetch_add(ticks as usize, Release);
    }

    /// Advance the timer by `ticks`, potentially waking any `Sleep` futures
    /// that have completed.
    ///
    /// # Interrupt Safety
    ///
    /// This method will *never* spin if the timer wheel lock is held; instead,
    /// it will add any new ticks to a counter of "pending" ticks and return
    /// immediately. Therefore, it is safe to call this method in an interrupt
    /// handler, as it will never acquire a lock that may already be locked.
    ///
    /// The [`force_advance`] method will spin to lock the timer wheel lock if
    /// it is currently held, *ensuring* that any pending wakeups are processed.
    /// That method should never be called in an interrupt handler.
    ///
    /// If a timer is driven primarily by calling `advance` in an interrupt
    /// handler, it may be desirable to occasionally call [`force_advance`]
    /// *outside* of an interrupt handler (i.e., as as part of an occasional
    /// runtime bookkeeping process). This ensures that any pending ticks are
    /// observed by the timer in a relatively timely manner.
    #[inline]
    pub fn advance(&self, ticks: Ticks) {
        // `advance` may be called in an ISR, so it can never actually spin.
        // instead, if the timer wheel is busy (e.g. the timer ISR was called on
        // another core, or if a `Sleep` future is currently canceling itself),
        // we just add to a counter of pending ticks, and bail.
        if let Some(core) = self.core.try_lock() {
            self.advance_locked(core, ticks);
        } else {
            // if the core of the timer wheel is already locked, add to the pending
            // tick count, which we will then advance the wheel by when it becomes
            // available.
            // TODO(eliza): if pending ticks overflows that's probably Bad News
            self.pend_ticks(ticks)
        }
    }

    /// Advance the timer by `ticks`, ensuring any `Sleep` futures that have
    /// completed are woken, even if a lock must be acquired.
    ///
    /// # Interrupt Safety
    ///
    /// This method will spin to acquire the timer wheel lock if it is currently
    /// held elsewhere. Therefore, this method must *NEVER* be called in an
    /// interrupt handler!
    ///
    /// If a timer is advanced inside an interrupt handler, use the [`advance`]
    /// method instead. If a timer is advanced primarily by calls to
    /// [`advance`], it may be desirable to occasionally call `force_advance`
    /// outside an interrupt handler, to ensure that pending ticks are drained
    /// frequently.
    pub fn force_advance(&self, ticks: Ticks) {
        self.advance_locked(self.core.lock(), ticks)
    }

    fn advance_locked(&self, mut core: MutexGuard<'_, Core>, ticks: Ticks) {
        // take any pending ticks.
        let pending_ticks = self.pending_ticks.swap(0, AcqRel) as Ticks;
        // we do two separate `advance` calls here instead of advancing once
        // with the sum, because `ticks` + `pending_ticks` could overflow.
        if pending_ticks > 0 {
            core.advance(pending_ticks);
        }
        core.advance(ticks);
    }

    #[cfg(test)]
    fn reset(&self) {
        let mut core = self.core.lock();
        *core = Core::new();
        self.pending_ticks.store(0, Release);
    }
}

// === impl Core ===

impl Core {
    const WHEELS: usize = Wheel::BITS;
    const fn new() -> Self {
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
            elapsed: 0,
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
    fn advance(&mut self, ticks: Ticks) -> usize {
        let now = self.elapsed + ticks;
        let mut fired = 0;
        let mut pending_reschedule = List::<sleep::Entry>::new();
        while let Some(deadline) = self.next_deadline() {
            if deadline.ticks > now {
                break;
            }

            let mut fired_this_turn = 0;
            let mut entries = self.wheels[deadline.wheel].take(deadline.slot);
            debug!(
                now = self.elapsed,
                deadline.ticks,
                entries = entries.len(),
                "turning wheel to"
            );

            while let Some(entry) = entries.pop_front() {
                let entry_deadline = unsafe { entry.as_ref().deadline.with(|deadline| *deadline) };

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

            trace!(at = self.elapsed, firing = fired_this_turn, "firing timers");

            self.elapsed = deadline.ticks;
            fired += fired_this_turn;
        }

        self.elapsed = now;

        // reschedule pending sleeps.
        debug!(
            now = self.elapsed,
            fired,
            rescheduled = pending_reschedule.len(),
            "wheel turned to"
        );
        while let Some(entry) = pending_reschedule.pop_front() {
            let deadline = unsafe { entry.as_ref().deadline.with(|deadline| *deadline) };
            debug_assert_ne!(deadline, 0);
            self.insert_sleep_at(deadline, entry)
        }

        fired
    }

    pub(super) fn cancel_sleep(&mut self, sleep: Pin<&mut sleep::Entry>) {
        let deadline = {
            let entry = sleep.as_ref().project_ref();
            let deadline = entry.deadline.with(|deadline| unsafe {
                // safety: this is safe because we are holding the lock on the
                // wheel.
                *deadline
            });
            trace!(
                sleep.addr = ?format_args!("{:p}", sleep),
                sleep.ticks = *entry.ticks,
                sleep.deadline = deadline,
                now = self.elapsed,
                "canceling sleep"
            );
            deadline
        };
        let wheel = self.wheel_index(deadline);
        self.wheels[wheel].remove(deadline, sleep);
    }

    pub(super) fn register_sleep(&mut self, mut sleep: ptr::NonNull<sleep::Entry>) {
        let deadline = unsafe {
            // safety: it's safe to access the entry's `start_time` field
            // mutably because `insert_sleep` is only called when the entry
            // hasn't been registered yet, and we are holding the timer lock.
            let entry = sleep.as_mut();
            let deadline = entry.ticks + self.elapsed;
            // set the entry's deadline with the wheel's current time.
            entry.deadline.with_mut(|entry_deadline| {
                debug_assert_eq!(
                    *entry_deadline, 0,
                    "sleep entry already bound to wheel! this is bad news!"
                );
                *entry_deadline = deadline
            });

            trace!(
                sleep.addr = ?sleep,
                sleep.ticks = entry.ticks,
                sleep.start_time = self.elapsed,
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
    fn next_deadline(&self) -> Option<timer::Deadline> {
        self.wheels.iter().find_map(|wheel| {
            let next_deadline = wheel.next_deadline(self.elapsed)?;
            trace!(
                now = self.elapsed,
                next_deadline.ticks,
                next_deadline.wheel,
                next_deadline.slot,
            );
            Some(next_deadline)
        })
    }

    #[inline]
    fn wheel_index(&self, ticks: Ticks) -> usize {
        wheel_index(self.elapsed, ticks)
    }
}

fn wheel_index(now: Ticks, ticks: Ticks) -> usize {
    const WHEEL_MASK: u64 = (1 << Wheel::BITS) - 1;

    // mask out the bits representing the index in the wheel
    let mut wheel_indices = now ^ ticks | WHEEL_MASK;

    // put sleeps over the max duration in the top level wheel
    if wheel_indices >= Timer::MAX_SLEEP_TICKS {
        wheel_indices = Timer::MAX_SLEEP_TICKS - 1;
    }

    let zeros = wheel_indices.leading_zeros();
    let rest = u64::BITS - 1 - zeros;

    rest as usize / Core::WHEELS
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
            !list.is_empty(),
            "if a slot is occupied, its list must not be empty"
        );
        self.clear_slot(slot);
        list
    }

    pub(super) fn next_deadline(&self, now: u64) -> Option<Deadline> {
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
    pub(super) fn next_slot_distance(&self, now: Ticks) -> Option<usize> {
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
