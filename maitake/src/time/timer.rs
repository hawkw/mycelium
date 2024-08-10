//! A [`Timer`] tracks the current time, and notifies [`Sleep`] and [`Timeout`]
//! [future]s when they complete.
//!
//! See the [`Timer`] type's documentation for details.
//!
//! [`Sleep`]: crate::time::Sleep
//! [`Timeout`]: crate::time::Timeout
//! [future]: core::future::Future
use super::clock::{self, Clock, Instant, Ticks};
use crate::{
    loom::sync::atomic::{AtomicUsize, Ordering::*},
    util::expect_display,
};
use core::time::Duration;
use maitake_sync::blocking::Mutex;
use mycelium_util::fmt;

#[cfg(test)]
mod tests;

pub(super) mod global;
pub(super) mod sleep;
mod wheel;

pub use self::global::{set_global_timer, AlreadyInitialized};
use self::sleep::Sleep;

/// A `Timer` tracks the current time, and notifies [`Sleep`] and [`Timeout`]
/// [future]s when they complete.
///
/// This timer implementation uses a [hierarchical timer wheel][wheel] to track
/// large numbers of `Sleep` futures efficiently.
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
/// system, a `Timer` instance cannot automatically advance time. Instead, the
/// operating system's run loop must periodically call the [`Timer::turn`]
/// method, which advances the timer wheel to the current time and wakes any
/// futures whose timers have completed.
///
/// The [`Timer::turn`] method may be called on a periodic interrupt, or on
/// every iteration of a system run loop. If the system is also using the
/// [`maitake::scheduler`](crate::scheduler) module, calling [`Timer::turn`]
/// after a call to [`Scheduler::tick`] is generally appropriate.
///
/// # Clocks
///
/// In addition to driving the timer, user code must also provide a [`Clock`]
/// when constructing a [`Timer`]. The [`Clock`] is a representation of a
/// hardware time source used to determine the current timestamp. See the
/// [`Clock`] type's documentation for details on implementing clocks.
///
/// ## Hardware Time Sources
///
/// Depending on the hardware platform, a time source may be a timer interrupt
/// that fires on a known interval[^1], or a timestamp that's read by reading
/// from a special register[^2], a memory-mapped IO location, or by executing a
/// special instruction[^3]. A combination of multiple time sources can also be
/// used.
///
/// [^1]: Such as the [8253 PIT interrupt] on most x86 systems.
///
/// [^2]: Such as the [`CCNT` register] on ARMv7.
///
/// [^3]: Such as the [`rdtsc` instruction] on x86_64.
///
/// ### Periodic and One-Shot Timer Interrupts
///
/// Generally, hardware timer interrupts operate in one of two modes: _periodic_
/// timers, which fire on a regular interval, and _one-shot_ timers, where the
/// timer counts down to a particular time, fires the interrupt, and then stops
/// until it is reset by software. Depending on the particular hardware
/// platform, one or both of these timer modes may be available.
///
/// Using a periodic timer with the `maitake` timer wheel is quite simple:
/// construct the timer wheel with the minimum [granularity](#timer-granularity)
/// set to the period of the timer interrupt, and call [`Timer::try_turn`] every
/// time the periodic interrupt occurs.
///
/// However, if the hardware platform provides a way to put the processor in a
/// low-power state while waiting for an interrupt, it may be desirable to
/// instead use a one-shot timer mode. When a timer wheel is advanced, it
/// returns a [`Turn`] structure describing what happened while advancing the
/// wheel. Among other things, this includes [the duration until the next
/// scheduled timer expires](Turn::time_to_next_deadline). If the timer is
/// advanced when the system has no other work to perform, and no new work was
/// scheduled as a result of advancing the timer wheel to the current time, the
/// system can then instruct the one-shot timer to fire in
/// [`Turn::time_to_next_deadline`], and put the processor in a low-power state
/// to wait for that interrupt. This allows the system to idle more efficiently
/// than if it was woken repeatedly by a periodic timer interrupt.
///
/// ### Timestamp-Driven Timers
///
/// When the timer is advanced by reading from a time source, the
/// [`Timer::turn`] method should be called periodically as part of a runtime
/// loop (as discussed in he previous section), such as every time [the
/// scheduler is ticked][`Scheduler::tick`]. Advancing the timer more frequently
/// will result in [`Sleep`] futures firing with a higher resolution, while less
/// frequent calls to [`Timer::turn`] will result in more noise in when [`Sleep`]
/// futures actually complete.
///
/// ## Timer Granularity
///
/// Within the timer wheel, the duration of a [`Sleep`] future is represented as
/// a number of abstract "timer ticks". The actual duration in real life time
/// that's represented by a number of ticks depends on the timer's _granularity.
///
/// When constructing `Timer` (e.g. by calling [`Timer::new`]), the minimum
/// granularity of the timer is determined by the [`Clock`]'s `tick_duration`
/// value. The selected tick duration influences both the resolution of the
/// timer (i.e. the minimum difference in duration between two `Sleep` futures
/// that can be distinguished by the timer), and the maximum duration that can
/// be represented by a `Sleep` future (which is limited by the size of a 64-bit
/// integer).
///
/// A longer tick duration will allow represented longer sleeps, as the maximum
/// allowable sleep is the timer's granularity multiplied by [`u64::MAX`]. A
/// shorter tick duration will allow for more precise sleeps at the expense of
/// reducing the maximum allowed sleep.
///
/// [`Sleep`]: crate::time::Sleep
/// [`Timeout`]: crate::time::Timeout
/// [future]: core::future::Future
/// [wheel]: http://www.cs.columbia.edu/~nahum/w6998/papers/sosp87-timing-wheels.pdf
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
    clock: Clock,

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

/// Represents a single turn of the timer wheel.
#[derive(Debug)]
pub struct Turn {
    /// The total number of ticks elapsed since the first time this timer wheel was
    /// advanced.
    pub now: Ticks,

    /// The tick at which the next deadline in the timer wheel expires.
    ///
    /// If this is `None`, there are currently no scheduled [`Sleep`] futures in the
    /// wheel.
    next_deadline_ticks: Option<Ticks>,

    /// The number of [`Sleep`] futures that were woken up by this turn of the
    /// timer wheel.
    pub expired: usize,

    /// The timer's tick duration (granularity); used for converting `now_ticks`
    /// and `next_deadline_ticks` into [`Duration`].
    tick_duration: Duration,
}

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
        /// Returns a new `Timer` with the specified hardware [`Clock`].
        #[must_use]
        pub fn new(clock: Clock) -> Self {
            Self {
                clock,
                pending_ticks: AtomicUsize::new(0),
                core: Mutex::new(wheel::Core::new()),
            }
        }
    }

    /// Returns the current timestamp according to this timer's hardware
    /// [`Clock`], as an [`Instant`].
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Borrows the hardware [`Clock`] definition used by this timer.
    #[must_use]
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Returns the maximum duration of [`Sleep`] futures driven by this timer.
    pub fn max_duration(&self) -> Duration {
        self.clock.max_duration()
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

    /// Attempt to turn the timer to the current `now` if the timer is not
    /// locked, potentially waking any [`Sleep`] futures that have completed.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(`[`Turn`]`)` if the lock was acquired and the wheel was
    ///   advanced. A [`Turn`] structure describes what occurred during this
    ///   turn of the wheel, including the [current time][elapsed] and the
    ///   [deadline of the next expiring timer][next], if one exists.
    /// - [`None`] if the wheel was not advanced because the lock was already
    ///   held.
    ///
    /// [elapsed]: Turn::elapsed
    /// [next]: Turn::time_to_next_deadline
    ///
    /// # Interrupt Safety
    ///
    /// This method will *never* spin if the timer wheel lock is held; instead,
    /// it will add any new ticks to a counter of "pending" ticks and return
    /// immediately. Therefore, it is safe to call this method in an interrupt
    /// handler, as it will never acquire a lock that may already be locked.
    ///
    /// The [`Timer::turn`] method will spin to lock the timer wheel lock if
    /// it is currently held, *ensuring* that any pending wakeups are processed.
    /// That method should never be called in an interrupt handler.
    ///
    /// If a timer is driven primarily by calling `try_turn` in an interrupt
    /// handler, it may be desirable to occasionally call [`Timer::turn`]
    /// *outside* of an interrupt handler (i.e., as as part of an occasional
    /// runtime bookkeeping process). This ensures that any pending ticks are
    /// observed by the timer in a relatively timely manner.
    #[inline]
    pub fn try_turn(&self) -> Option<Turn> {
        // `try_turn` may be called in an ISR, so it can never actually spin.
        // instead, if the timer wheel is busy (e.g. the timer ISR was called on
        // another core, or if a `Sleep` future is currently canceling itself),
        // we just add to a counter of pending ticks, and bail.
        self.core.try_with_lock(|core| self.advance_locked(core))
    }

    /// Advance the timer to the current time, ensuring any [`Sleep`] futures that
    /// have completed are woken, even if a lock must be acquired.
    ///
    /// # Returns
    ///
    /// A [`Turn`] structure describing what occurred during this turn of the
    /// wheel, including including the [current time][elapsed] and the [deadline
    /// of the next expiring timer][next], if one exists.
    ///
    /// [elapsed]: Turn::elapsed
    /// [next]: Turn::time_to_next_deadline
    ///
    /// # Interrupt Safety
    ///
    /// This method will spin to acquire the timer wheel lock if it is currently
    /// held elsewhere. Therefore, this method must *NEVER* be called in an
    /// interrupt handler!
    ///
    /// If a timer is advanced inside an interrupt handler, use the
    /// [`Timer::try_turn`] method instead. If a timer is advanced primarily by
    /// calls to [`Timer::try_turn`] in an interrupt handler, it may be
    /// desirable to occasionally call `turn` outside an interrupt handler, to
    /// ensure that pending ticks are drained frequently.
    #[inline]
    pub fn turn(&self) -> Turn {
        self.core.with_lock(|core| self.advance_locked(core))
    }

    pub(in crate::time) fn ticks_to_dur(&self, ticks: Ticks) -> Duration {
        clock::ticks_to_dur(self.clock.tick_duration(), ticks)
    }

    pub(in crate::time) fn dur_to_ticks(&self, duration: Duration) -> Result<Ticks, TimerError> {
        clock::dur_to_ticks(self.clock.tick_duration(), duration)
    }

    fn advance_locked(&self, core: &mut wheel::Core) -> Turn {
        // take any pending ticks.
        let pending_ticks = self.pending_ticks.swap(0, AcqRel) as Ticks;
        // we do two separate `advance` calls here instead of advancing once
        // with the sum, because `ticks` + `pending_ticks` could overflow.
        let mut expired: usize = 0;
        if pending_ticks > 0 {
            let (expiring, _next_deadline) = core.turn_to(pending_ticks);
            expired = expired.saturating_add(expiring);
        }

        let mut now = self.clock.now_ticks();
        loop {
            let (expiring, next_deadline) = core.turn_to(now);
            expired = expired.saturating_add(expiring);
            if let Some(next) = next_deadline {
                now = self.clock.now_ticks();
                if now >= next.ticks {
                    // we've advanced past the next deadline, so we need to
                    // advance again.
                    continue;
                }
            }

            return Turn {
                expired,
                next_deadline_ticks: next_deadline.map(|d| d.ticks),
                now,
                tick_duration: self.clock.tick_duration(),
            };
        }
    }

    #[cfg(all(test, not(loom)))]
    fn reset(&self) {
        self.core.with_lock(|core| {
            *core = wheel::Core::new();
            self.pending_ticks.store(0, Release);
        });
    }
}

impl fmt::Debug for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            clock,
            pending_ticks,
            core,
        } = self;
        let mut s = f.debug_struct("Timer");
        s.field("clock", &clock)
            .field("tick_duration", &clock.tick_duration())
            .field("pending_ticks", &pending_ticks.load(Acquire));
        core.try_with_lock(|core| s.field("core", &core).finish())
            .unwrap_or_else(|| s.field("core", &"<locked>").finish())
    }
}

// === impl Turn ===

impl Turn {
    /// Returns the number of ticks until the deadline at which the next
    /// [`Sleep`] future expires, or [`None`] if no sleep futures are currently
    /// scheduled.
    #[inline]
    #[must_use]
    pub fn ticks_to_next_deadline(&self) -> Option<Ticks> {
        self.next_deadline_ticks.map(|deadline| deadline - self.now)
    }

    /// Returns the [`Duration`] from the current time to the deadline of the
    /// next [`Sleep`] future, or [`None`] if no sleep futures are currently
    /// scheduled.
    #[inline]
    #[must_use]
    pub fn time_to_next_deadline(&self) -> Option<Duration> {
        self.ticks_to_next_deadline()
            .map(|deadline| clock::ticks_to_dur(self.tick_duration, deadline))
    }

    /// Returns the total elapsed time since this timer wheel started running.
    #[inline]
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        clock::ticks_to_dur(self.tick_duration, self.now)
    }

    /// Returns `true` if there are currently pending [`Sleep`] futures
    /// scheduled in this timer wheel.
    #[inline]
    #[must_use]
    pub fn has_remaining(&self) -> bool {
        self.next_deadline_ticks.is_some()
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

feature! {
    #![feature = "core-error"]
    impl core::error::Error for TimerError {}
}
