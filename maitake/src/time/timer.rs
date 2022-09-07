use crate::loom::sync::{
    atomic::{AtomicUsize, Ordering::*},
    spin::{Mutex, MutexGuard},
};
use core::{convert::TryFrom, time::Duration};
use mycelium_util::fmt;

#[cfg(all(test, loom))]
mod loom;
#[cfg(all(test, not(loom)))]
mod tests;

pub(super) mod sleep;
mod wheel;

use self::sleep::Sleep;

#[derive(Debug)]
pub struct Timer {
    tick_duration: Duration,
    tick_timer: TickTimer,
}

pub struct TickTimer {
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

    core: Mutex<wheel::Core>,
}

pub type Ticks = u64;

// === impl Timer ===

impl Timer {
    loom_const_fn! {
        pub fn new(tick_duration: Duration) -> Self {
            Self {
                tick_duration,
                tick_timer: TickTimer::new(),
            }
        }
    }

    pub fn max_duration(&self) -> Duration {
        self.ticks_to_dur(u64::MAX)
    }

    /// Returns a future that will complete in `duration`.
    #[track_caller]
    pub fn sleep(&self, duration: Duration) -> Sleep<'_> {
        self.sleep_ticks(self.dur_to_ticks(duration))
    }

    /// Returns a future that will complete in `ticks` timer ticks.
    // XXX(eliza): should this be a public API?
    #[track_caller]
    fn sleep_ticks(&self, ticks: Ticks) -> Sleep<'_> {
        self.tick_timer.sleep(ticks)
    }

    /// Add pending time to the timer *without* turning the wheel.
    ///
    /// This function will *never* acquire a lock, and will *never* notify any
    /// waiting [`Sleep`] futures. It can be called in an interrupt handler that
    /// cannot perform significant amounts of work.
    ///
    /// However, if this method is used, then [`Timer::force_advance`] must be
    /// called frequently from outside of the interrupt handler.
    #[inline(always)]
    pub fn pend_duration(&self, duration: Duration) {
        self.pend_ticks(self.dur_to_ticks(duration))
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
        self.tick_timer.pend_ticks(ticks)
    }

    /// Advance the timer by `duration`, potentially waking any `Sleep` futures
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
    pub fn advance(&self, duration: Duration) {
        self.advance_ticks(self.dur_to_ticks(duration))
    }

    /// Advance the timer by `ticks` timer ticks, potentially waking any `Sleep`
    /// futures that have completed.
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
    /// handler, it may be desirable to occasionally call [`force_advance_ticks`]
    /// *outside* of an interrupt handler (i.e., as as part of an occasional
    /// runtime bookkeeping process). This ensures that any pending ticks are
    /// observed by the timer in a relatively timely manner.
    #[inline]
    pub fn advance_ticks(&self, ticks: Ticks) {
        self.tick_timer.advance(ticks)
    }

    /// Advance the timer by `duration`, ensuring any `Sleep` futures that have
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
    #[inline]
    pub fn force_advance(&self, duration: Duration) {
        self.force_advance_ticks(self.dur_to_ticks(duration))
    }

    /// Advance the timer by `ticks` timer ticks, ensuring any `Sleep` futures
    /// that have completed are woken, even if a lock must be acquired.
    ///
    /// # Interrupt Safety
    ///
    /// This method will spin to acquire the timer wheel lock if it is currently
    /// held elsewhere. Therefore, this method must *NEVER* be called in an
    /// interrupt handler!
    ///
    /// If a timer is advanced inside an interrupt handler, use the [`advance_ticks`]
    /// method instead. If a timer is advanced primarily by calls to
    /// [`advance_ticks`], it may be desirable to occasionally call `force_advance`
    /// outside an interrupt handler, to ensure that pending ticks are drained
    /// frequently.
    #[inline]
    pub fn force_advance_ticks(&self, ticks: Ticks) {
        self.tick_timer.force_advance(ticks)
    }

    #[track_caller]
    fn dur_to_ticks(&self, dur: Duration) -> Ticks {
        match (dur.as_nanos() / self.tick_duration.as_nanos()).try_into() {
            Ok(ticks) => ticks,
            Err(_) => panic!(
                "duration {dur:?} is too large to convert into timer ticks (this timer's max duration is {:?})",
                self.max_duration()
            ),
        }
    }

    #[track_caller]
    fn ticks_to_dur(&self, ticks: Ticks) -> Duration {
        let nanos = self.tick_duration.subsec_nanos() as u64 * ticks;
        let secs = self.tick_duration.as_secs() * ticks;
        Duration::new(secs, nanos as u32)
    }
}

// === impl TickTimer ==

impl TickTimer {
    loom_const_fn! {
        pub fn new() -> Self {
            Self {
                pending_ticks: AtomicUsize::new(0),
                core: Mutex::new(wheel::Core::new()),
            }
        }
    }

    /// Returns a future that will complete in `ticks` timer ticks.
    #[track_caller]
    pub fn sleep(&self, ticks: Ticks) -> Sleep<'_> {
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

    fn advance_locked(&self, mut core: MutexGuard<'_, wheel::Core>, ticks: Ticks) {
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
        *core = wheel::Core::new();
        self.pending_ticks.store(0, Release);
    }
}

impl fmt::Debug for TickTimer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TickTimer")
            .field("pending_ticks", &self.pending_ticks.load(Acquire))
            .field("core", &fmt::opt(&self.core.try_lock()).or_else("<locked>"))
            .finish()
    }
}
