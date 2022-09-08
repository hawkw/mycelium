//! A [`Timer`] tracks the current time, and notifies [`Sleep`] and [`Timeout`]
//! [future]s when they complete.
//!
//! See the [`Timer`] type's documentation for details.
//!
//! [`Sleep`]: crate::time::Sleep
//! [`Timeout`]: crate::time::Timeout
//! [future]: core::future::Future
use crate::{
    loom::sync::{
        atomic::{AtomicUsize, Ordering::*},
        spin::{Mutex, MutexGuard},
    },
    util::expect_display,
};
use core::time::Duration;
use mycelium_util::fmt;

#[cfg(all(test, loom))]
mod loom;
#[cfg(all(test, not(loom)))]
mod tests;

pub(super) mod global;
pub(super) mod sleep;
mod wheel;

pub use self::global::{set_global_timer, AlreadyInitialized};
use self::sleep::Sleep;

/// A `Timer` tracks the current time, and notifies [`Sleep`] and [`Timeout`]
/// [future]s when they complete.
///
/// # Creating Futures
///
/// A `Timer` instance is necessary to create [`Sleep`] and [`Timeout`] futures.
/// Once a [`Sleep`] or [`Timeout`] future is created by a `Timer`, they are
/// *bound* to that `Timer` instance, and will be woken by the `Timer` once it
/// advances past the deadline for that future.
///
/// The [`Timer::sleep`] and [`Timer::timeout`] methods create [`Sleep`] and
/// [`Timeout`] futures, respectively. In addition, fallible
/// [`Timer::try_sleep`] and [`Timer::try_timeout`] methods are available, which
/// do not panic on invalid durations. These methods may be used in systems
/// where panicking must be avoided.
///
/// ### Setting a Global Timer
///
/// In addition to creating [`Sleep`] and [`Timeout`] futures using methods on a
/// `Timer` instance, a timer may also be set as a [global default timer]. This
/// allows the use of the free functions [`sleep`], [`timeout`],
/// [`try_sleep`], and [`try_timeout`], which do not require a reference to a
/// `Timer` to be passed in. See [the documentation on global timers][global]
/// for details.
///
/// # Driving Timers
///
/// &#x26a0;&#xfe0f; *A timer wheel at rest will remain at rest unless acted
/// upon by an outside force!*
///
/// Since `maitake` is intended for bare-metal platforms without an operating
/// system, a `Timer` instance cannot automatically advance time. Instead, it
/// must be driven by a *time source*, which calls the [`Timer::advance`] method
/// and/or the [`Timer::pend_duration`] and [`Timer::force_advance`] methods.
///
/// Depending on the hardware platform, a time source may be a timer interrupt
/// that fires on a known interval[^1], or a timestamp that's read by reading
/// from a special register[^2], a memory-mapped IO location, or by executing a
/// special instruction[^3]. A combination of multiple time sources can also be
/// used.
///
/// In any case, the timer must be advanced periodically by the time source.
///
/// [^1]: Such as the [8253 PIT interrupt] on most x86 systems.
///
/// [^2]: Such as the [`CCNT` register] on ARMv7.
///
/// [^3]: Such as the [`rdtsc` instruction] on x86_64.
///
/// ### Interrupt-Driven Timers
///
/// When the timer is interrupt-driven, the interrupt handler for the timer
/// interrupt should call either the [`Timer::pend_duration`] or
/// [`Timer::advance`] methods.
///
/// [`Timer::advance`] will attempt to optimistically acquire a spinlock, and
/// advance the timer if it is acquired, or add to the pending tick counter if
/// the timer wheel is currently locked. Therefore, it is safe to call in an
/// interrupt handler, as it and cannot cause a deadlock.
///
/// However, if interrupt handlers must be extremely short, the
/// [`Timer::pend_duration`] method can be used, instead. This method will
/// *never* acquire a lock, and does not actually turn the timer wheel. Instead,
/// it always performs only a single atomic add. If the time source is an
/// interrupt handler which calls [`Timer::pend_duration`], though, the timer
/// wheel must be turned externally. This can be done by calling the
/// [`Timer::force_advance_ticks`] method periodically outside of the interrupt
/// handler, with a duration of 0 ticks. In general, this should be done as some
/// form of runtime bookkeeping action. For example, the timer can be advanced
/// in a system's run loop every time the [`Scheduler::tick`] method completes.
///
/// ### Timestamp-Driven Timers
///
/// When the timer is advanced by reading from a time source, the
/// [`Timer::advance`] method should generally be used to drive the timer. Prior
/// to calling [`Timer::advance`], the time source is read to determine the
/// duration that has elapsed since the last time [`Timer::advance`] was called,
/// and that duration is provided when calling `advance`.
///
/// This should occur periodically as part of a runtime loop (as discussed in
/// the previous section), such as every time [the scheduler is
/// ticked][`Scheduler::tick`]. Advancing the timer more frequently will result
/// in [`Sleep`] futures firing with a higher resolution, while less frequent
/// calls to [`Timer::advance`] will result in more noise in when [`Sleep`]
/// futures actually complete.
///
/// # Timer Granularity
///
/// TODO(eliza): write this part
///
///
/// [`Sleep`]: crate::time::Sleep
/// [`Timeout`]: crate::time::Timeout
/// [future]: core::future::Future
/// [8253 PIT interrupt]: https://en.wikipedia.org/wiki/Intel_8253#IBM_PC_programming_tips_and_hints
/// [`CCNT` register]: https://developer.arm.com/documentation/ddi0211/h/system-control-coprocessor/system-control-processor-register-descriptions/c15--cycle-counter-register--ccnt-
/// [`rdtsc` instruction]: https://www.felixcloutier.com/x86/rdtsc
/// [`Scheduler::tick`]: crate::scheduler::Scheduler::tick
/// [`sleep`]: crate::time::sleep()
/// [`timeout`]: crate::time::timeout()
/// [`try_sleep`]: crate::time::try_sleep()
/// [`try_timeout`]: crate::time::try_timeout()
/// [global]: crate::time#global-timers
pub struct Timer {
    /// The duration represented by one tick of this timer.
    ///
    /// This represents the timer's finest granularity; durations shorter than
    /// this are rounded up to one tick.
    tick_duration: Duration,

    /// A count of how many timer ticks have elapsed since the last time the
    /// timer's [`Core`] was updated.
    ///
    /// The timer's [`advance`] method may be called in an interrupt
    /// handler, so it cannot spin to lock the `Core` if it is busy. Instead, it
    /// tries to acquire the [`Core`] lock, and if it can't, it increments
    /// `pending_ticks`. The count of pending ticks is then consumed the next
    /// time the timer interrupt is able to lock the [`Core`].
    ///
    /// This strategy may result in some additional noise in when exactly a
    /// sleep will fire, but it allows us to avoid potential deadlocks when the
    /// timer is advanced from an interrupt handler.
    ///
    /// [`Core`]: wheel::Core
    /// [`advance`]: Timer::advance
    pending_ticks: AtomicUsize,

    /// The hierarchical timer wheel.
    ///
    /// This is stored inside a [`Mutex`], which must be locked in order to
    /// register a new [`Sleep`] future, cancel a [`Sleep`] that is currently
    /// registered, and turn the timer wheel. `pending_ticks` is outside this
    /// lock, in order to allow incrementing the time that has advanced in an
    /// ISR without locking. Pending ticks are then consumed by locking the
    /// wheel and advancing the current time, which can be done either
    /// optimistically (*try* to consume any pending ticks when the lock is not
    /// busy), or forcefully (spin to acquire the lock and then ensure all
    /// pending ticks are *definitely* consumed).
    ///
    /// XXX(eliza): would be nice if the "elapsed" counter could be moved
    /// outside the lock so we can have a global `Instant::now()` without
    /// locking...but that's hard to do without 64-bit atomics...
    ///
    /// Also, we could consider trying to make locking more granular here so
    /// that we lock individual wheels in order to register a sleep, but that
    /// would be Complicated...and then we'd need some way to tell a sleep
    /// future "you are on a different wheel now, so here is what you would have
    /// to lock if you need to cancel yourself"...
    core: Mutex<wheel::Core>,
}

/// Timer ticks are always counted by a 64-bit unsigned integer.
pub type Ticks = u64;

/// Errors returned by [`Timer::try_sleep`], [`Timer::try_timeout`], and the
/// global [`try_sleep`] and [`try_timeout`] functions.
///
/// [`try_sleep`]: super::try_sleep
/// [`try_timeout`]: super::try_timeout
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TimerError {
    /// No [global default timer][global] has been set.
    ///
    /// This error is returned by the [`try_sleep`] and [`try_timeout`]
    /// functions only.
    ///
    /// [global]: super#global-timers
    /// [`try_sleep`]: super::try_sleep
    /// [`try_timeout`]: super::try_timeout
    NoGlobalTimer,
    /// The requested [`Duration`] exceeds the [timer's maximum duration][max].
    ///
    /// This error is returned by [`Timer::try_sleep`], [`Timer::try_timeout`],
    /// and the global [`try_sleep`] and [`try_timeout`] functions.
    ///
    /// [`try_sleep`]: super::try_sleep
    /// [`try_timeout`]: super::try_timeout
    /// [max]: Timer::max_duration
    DurationTooLong {
        /// The duration that was requested for a [`Sleep`] or [`Timeout`]
        /// future.
        ///
        /// [`Timeout`]: crate::time::Timeout
        requested: Duration,
        /// The [maximum duration][max] supported by this [`Timer`] instance.
        ///
        /// [max]: Timer::max_duration
        max: Duration,
    },
}

// === impl Timer ===

impl Timer {
    loom_const_fn! {
        /// Returns a new `Timer` with the specified `tick_duration` for a single timer
        /// tick.
        #[must_use]
        pub fn new(tick_duration: Duration) -> Self {
            Self {
                tick_duration,
                pending_ticks: AtomicUsize::new(0),
                core: Mutex::new(wheel::Core::new()),
            }
        }
    }

    /// Returns the maximum duration of [`Sleep`] futures driven by this timer.
    pub fn max_duration(&self) -> Duration {
        self.ticks_to_dur(u64::MAX)
    }

    /// Returns a [`Future`] that will complete in `duration`.
    ///
    /// # Returns
    ///
    /// The returned [`Sleep`] future will be driven by this timer, and will
    /// complete once this timer has advanced by at least `duration`.
    ///
    /// # Panics
    ///
    /// This method panics if the provided duration exceeds the [maximum sleep
    /// duration][max] allowed by this timer.
    ///
    /// For a version of this function that does not panic, see
    /// [`Timer::try_sleep`].
    ///
    /// [global]: #global-timers
    /// [max]: Timer::max_duration
    /// [`Future`]: core::future::Future
    #[track_caller]
    pub fn sleep(&self, duration: Duration) -> Sleep<'_> {
        expect_display(self.try_sleep(duration), "cannot create `Sleep` future")
    }

    /// Returns a [`Future`] that will complete in `duration`.
    ///
    /// # Returns
    ///
    /// - [`Ok`]`(`[`Sleep`]`)` if a new [`Sleep`] future was created
    ///   successfully.
    /// - [`Err`]`(`[`TimerError::DurationTooLong`]`)` if the requested sleep
    ///   duration exceeds this timer's [maximum sleep
    ///   duration](Timer::max_duration`).
    ///
    /// The returned [`Sleep`] future will be driven by this timer, and will
    /// complete once this timer has advanced by at least `duration`.
    ///
    /// # Panics
    ///
    /// This method does not panic. For a version of this method that panics
    /// rather than returning an error, see [`Timer::sleep`].
    ///
    /// [`Future`]: core::future::Future
    pub fn try_sleep(&self, duration: Duration) -> Result<Sleep<'_>, TimerError> {
        let ticks = self.dur_to_ticks(duration)?;
        Ok(self.sleep_ticks(ticks))
    }

    /// Returns a [`Future`] that will complete in `ticks` timer ticks.
    ///
    /// # Returns
    ///
    /// The returned [`Sleep`] future will be driven by this timer, and will
    /// complete once this timer has advanced by at least `ticks` timer ticks.
    ///
    /// [`Future`]: core::future::Future
    #[track_caller]
    pub fn sleep_ticks(&self, ticks: Ticks) -> Sleep<'_> {
        Sleep::new(self, ticks)
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
        let ticks = expect_display(
            self.dur_to_ticks(duration),
            "cannot add to pending duration",
        );
        self.pend_ticks(ticks)
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
    #[track_caller]
    pub fn pend_ticks(&self, ticks: Ticks) {
        debug_assert!(
            ticks < usize::MAX as u64,
            "cannot pend more than `usize::MAX` ticks at once!"
        );
        self.pending_ticks.fetch_add(ticks as usize, Release);
    }

    /// Advance the timer by `duration`, potentially waking any [`Sleep`] futures
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
    ///
    /// [`force_advance`]: Timer::force_advance
    #[inline]
    pub fn advance(&self, duration: Duration) {
        let ticks = expect_display(self.dur_to_ticks(duration), "cannot advance timer");
        self.advance_ticks(ticks)
    }

    /// Advance the timer by `ticks` timer ticks, potentially waking any [`Sleep`]
    /// futures that have completed.
    ///
    /// # Interrupt Safety
    ///
    /// This method will *never* spin if the timer wheel lock is held; instead,
    /// it will add any new ticks to a counter of "pending" ticks and return
    /// immediately. Therefore, it is safe to call this method in an interrupt
    /// handler, as it will never acquire a lock that may already be locked.
    ///
    /// The [`force_advance_ticks`] method will spin to lock the timer wheel lock if
    /// it is currently held, *ensuring* that any pending wakeups are processed.
    /// That method should never be called in an interrupt handler.
    ///
    /// If a timer is driven primarily by calling `advance` in an interrupt
    /// handler, it may be desirable to occasionally call [`force_advance_ticks`]
    /// *outside* of an interrupt handler (i.e., as as part of an occasional
    /// runtime bookkeeping process). This ensures that any pending ticks are
    /// observed by the timer in a relatively timely manner.
    ///
    /// [`force_advance_ticks`]: Timer::force_advance_ticks
    #[inline]
    pub fn advance_ticks(&self, ticks: Ticks) {
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
    ///
    /// [`advance`]: Timer::advance
    #[inline]
    pub fn force_advance(&self, duration: Duration) {
        let ticks = expect_display(self.dur_to_ticks(duration), "cannot advance timer");
        self.force_advance_ticks(ticks)
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
    ///
    /// [`advance_ticks`]: Timer::advance_ticks
    #[inline]
    pub fn force_advance_ticks(&self, ticks: Ticks) {
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

    fn core(&self) -> MutexGuard<'_, wheel::Core> {
        self.core.lock()
    }

    #[cfg(test)]
    fn reset(&self) {
        let mut core = self.core.lock();
        *core = wheel::Core::new();
        self.pending_ticks.store(0, Release);
    }

    #[track_caller]
    fn dur_to_ticks(&self, dur: Duration) -> Result<Ticks, TimerError> {
        (dur.as_nanos() / self.tick_duration.as_nanos())
            .try_into()
            .map_err(|_| TimerError::DurationTooLong {
                requested: dur,
                max: self.max_duration(),
            })
    }

    #[track_caller]
    fn ticks_to_dur(&self, ticks: Ticks) -> Duration {
        let nanos = self.tick_duration.subsec_nanos() as u64 * ticks;
        let secs = self.tick_duration.as_secs() * ticks;
        Duration::new(secs, nanos as u32)
    }
}

impl fmt::Debug for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timer")
            .field("tick_duration", &self.tick_duration)
            .field("pending_ticks", &self.pending_ticks.load(Acquire))
            .field("core", &fmt::opt(&self.core.try_lock()).or_else("<locked>"))
            .finish()
    }
}

// === impl TimerError ====

impl fmt::Display for TimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimerError::NoGlobalTimer => f.pad(
                "no global timer has been initialized! \
                `set_global_timer` must be called before calling \
                this function.",
            ),
            TimerError::DurationTooLong { requested, max } => write!(
                f,
                "requested duration {requested:?} exceeds this timer's \
                maximum duration ({max:?}."
            ),
        }
    }
}
