//! Timer utilities.
use crate::loom::sync::{
    atomic::{AtomicUsize, Ordering::*},
    spin::{Mutex, MutexGuard},
};
use core::{pin::Pin, ptr};
mod sleep;
mod wheel;

#[cfg(all(test, not(loom)))]
mod tests;

#[cfg(all(test, loom))]
mod loom;

pub use self::sleep::Sleep;
use self::wheel::Wheel;

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

struct Core {
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
    const MAX_SLEEP_TICKS: u64 = (1 << (wheel::BITS * Core::WHEELS)) - 1;

    /// Returns a future that will complete in `ticks` timer ticks.
    #[track_caller]
    pub fn sleep(&self, ticks: Ticks) -> Sleep<'_> {
        assert!(
            ticks < Self::MAX_SLEEP_TICKS,
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
    const WHEELS: usize = wheel::BITS;
    pub const fn new() -> Self {
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
        let mut now = self.elapsed;
        self.elapsed += ticks;
        let mut fired = 0;
        while let Some(deadline) = self.next_deadline(now) {
            if deadline.ticks > self.elapsed {
                break;
            }

            now = deadline.ticks;

            let mut entries = self.wheels[deadline.wheel].take(deadline.slot);
            let firing = entries.len();
            debug!(deadline.ticks, firing, "turning wheel to");
            fired += firing;
            while let Some(timer) = entries.pop_front() {
                unsafe {
                    // safety: this is safe because we are holding the lock on
                    // the timer `Core`.
                    timer.as_ref().fire();
                }
            }
        }
        debug!(now = self.elapsed, fired, "wheel turned to");
        fired
    }

    fn cancel_sleep(&mut self, sleep: Pin<&mut sleep::Entry>) {
        let deadline = {
            let entry = sleep.as_ref().project_ref();
            let ticks = *entry.ticks;
            let start = entry.start_time.with(|start| unsafe {
                // safety: this is safe because we are holding the lock on the
                // wheel.
                *start
            });
            let deadline = start + ticks;
            trace!(
                sleep.addr = ?format_args!("{:p}", sleep),
                sleep.ticks = ticks,
                sleep.start_time = start,
                sleep.deadline = deadline,
                now = self.elapsed,
                "canceling sleep"
            );
            deadline
        };
        let wheel = self.wheel_index(deadline);
        self.wheels[wheel].remove(deadline, sleep);
    }

    fn register_sleep(&mut self, mut sleep: ptr::NonNull<sleep::Entry>) {
        let ticks = unsafe {
            // safety: it's safe to access the entry's `start_time` field
            // mutably because `insert_sleep` is only called when the entry
            // hasn't been registered yet, and we are holding the timer lock.
            let entry = sleep.as_mut();

            // set the entry's start time to the wheel's current time.
            entry.start_time.with_mut(|start_time| {
                debug_assert_eq!(
                    *start_time, 0,
                    "sleep entry already bound to wheel! this is bad news!"
                );
                *start_time = self.elapsed
            });

            entry.ticks
        };
        let deadline = ticks + self.elapsed;
        trace!(
            sleep.addr = ?sleep,
            sleep.ticks = ticks,
            sleep.start_time = self.elapsed,
            sleep.deadline = deadline,
            "registering sleep"
        );
        let wheel = self.wheel_index(deadline);
        self.wheels[wheel].insert(deadline, sleep);
    }

    /// Returns the deadline and location of the next firing timer in the wheel.
    #[inline]
    fn next_deadline(&self, now: Ticks) -> Option<wheel::Deadline> {
        self.wheels.iter().find_map(|wheel| {
            let next_deadline = wheel.next_deadline(now)?;
            trace!(
                now,
                next_deadline.ticks,
                next_deadline.wheel,
                next_deadline.slot,
            );
            Some(next_deadline)
        })
    }

    #[inline]
    fn wheel_index(&self, ticks: Ticks) -> usize {
        const WHEEL_MASK: u64 = (1 << wheel::BITS) - 1;

        // mask out the bits representing the index in the wheel
        let wheel_indices = self.elapsed ^ ticks | WHEEL_MASK;
        let zeros = wheel_indices.leading_zeros();
        let rest = u64::BITS - 1 - zeros;

        rest as usize / Self::WHEELS
    }
}
