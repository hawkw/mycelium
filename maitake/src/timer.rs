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

    /// Returns a future that will complete in `ticks` timer ticks.
    pub fn sleep(&self, ticks: Ticks) -> Sleep<'_> {
        Sleep::new(&self.core, ticks)
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
        if let Some(mut core) = self.core.try_lock() {
            self.advance_locked(core, ticks);
        } else {
            // if the core of the timer wheel is already locked, add to the pending
            // tick count, which we will then advance the wheel by when it becomes
            // available.
            // TODO(eliza): if pending ticks overflows that's probably Bad News
            self.pending_ticks.fetch_add(ticks as usize, Release);
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

    fn advance_locked(&self, core: MutexGuard<'_, Core>, ticks: Ticks) {
        // take any pending ticks.
        let pending_ticks = self.pending_ticks.swap(0, AcqRel) as Ticks;
        // we do two separate `advance` calls here instead of advancing once
        // with the sum, because `ticks` + `pending_ticks` could overflow.
        if pending_ticks > 0 {
            core.advance(pending_ticks);
        }
        core.advance(ticks);
    }
}

// === impl Core ===

impl Core {
    const WHEELS: usize = wheel::BITS;
    pub const fn new() -> Self {
        // Used as an initializer when constructing a new `Core`.
        const NEW_WHEEL: Wheel = Wheel::new();
        Self {
            elapsed: 0,
            wheels: [NEW_WHEEL; Self::WHEELS],
        }
    }

    #[inline(never)]
    fn advance(&mut self, ticks: Ticks) -> usize {
        todo!("actually advance the timer wheel")
    }

    fn cancel_sleep(&mut self, sleep: Pin<&mut sleep::Entry>) {
        let ticks = *(sleep.as_ref().project_ref().ticks);
        let wheel = self.wheel_index(ticks);
        self.wheels[wheel].remove(wheel, ticks, sleep);
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
        let wheel = self.wheel_index(ticks);
        self.wheels[wheel].insert(wheel, ticks, sleep);
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
