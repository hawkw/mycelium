//! [`Clock`]s provide a mechanism for tracking the current time.
//!
//! See the documentation for the [`Clock`] type for more details.
use super::{
    timer::{self, TimerError},
    Duration,
};
use core::{
    fmt,
    ops::{Add, AddAssign, Sub, SubAssign},
};

/// A hardware clock definition.
///
/// A `Clock` consists of a function that returns the hardware clock's current
/// timestamp in [`Ticks`] (`now()`), and a [`Duration`] that defines the amount
/// of time represented by a single tick of the clock.
///
/// # Using `Clock`s
///
/// A `Clock` must be provided when [constructing a `Timer`](super::Timer::new).
/// The `Clock` provided to [`Timer::new`](super::Timer::new) is used to
/// determine the time to advance to when turning the timer wheel, and to
/// determine the start time when adding a [`Sleep`](super::Sleep) future to the
/// timer wheel.
///
/// In addition, once a global timer is set using the
/// [`set_global_timer`](super::set_global_timer) function, the
/// [`Instant::now()`] function may be used to produce [`Instant`]s representing
/// the current timestamp according to that timer's [`Clock`]. The [`Instant`]
/// type is analogous to the [`std::time::Instant`] type in the Rust standard
/// library, and may be used to compare the time elapsed between two events, as
/// a timestamp for logs and other diagnostics, and similar purposes.
///
/// # Implementing `now()`
///
/// Constructing a [new `Clock` definition](Self::new) takes a function, called
/// `now()`, that returns the current hardware timestamp in a 64-bit number of,
/// _ticks_. The period of time represented by a tick is indicated by the
/// `tick_duration` argument to [`Clock::new`]. In order to define a `Clock`
/// representing a particular hardware time source, a `now()` function must be
/// implemented using that time source.
///
/// ## Monotonicity
///
/// Implementations of `now()` MUST ensure that timestamps returned by
/// `now()` MUST be [monontonically non-decreasing][monotonic]. This means that
/// a call to `now()` MUST NOT ever return a value less than the value returned
/// by a previous call to`now()`.
///
/// Note that this means that timestamps returned by `now()` are expected
/// not to overflow. Of course, all integers *will* overflow eventually, so
/// this requirement can reasonably be weakened to expecting that timestamps
/// returned by `now()` will not overflow unless the system has been running
/// for a duration substantially longer than the system is expected to run
/// for. For example, if a system is expected to run for as long as a year
/// without being restarted, it's not unreasonable for timestamps returned
/// by `now()` to overflow after, say, 100 years. Ideally, a general-purpose
/// `Clock` implementation would not overflow for, say, 1,000 years.
///
/// The implication of this is that if the timestamp counters provided by
/// the hardware platform are less than 64 bits wide (e.g., 16- or 32-bit
/// timestamps), the `Clock` implementation is responsible for ensuring that
/// they are extended to 64 bits, such as by counting overflows in the
/// `Clock` implementation.
///
/// ## Examples
///
/// The simplest possible `Clock` implementation is one for a
/// timestamp-counter-style hardware clock, where the timestamp counter is
/// incremented on a fixed interval by the hardware. For such a counter, the
/// `Clock` definition might look something like this:
///
///```rust
/// use maitake::time::{Clock, Duration};
/// # mod arch {
/// #     pub fn read_timestamp_counter() -> u64 { 0 }
/// #     pub const TIMESTAMP_COUNTER_FREQUENCY_HZ: u64 = 50_000_000;
/// # }
///
/// // The fixed interval at which the timestamp counter is incremented.
/// //
/// // This is a made-up value; for real hardware, this value would be
/// // determined from information provided by the hardware manufacturer,
/// // and may need to be calculated based on the system's clock frequency.
/// const TICK_DURATION: Duration = {
///     let dur_ns = 1_000_000_000 / arch::TIMESTAMP_COUNTER_FREQUENCY_HZ;
///     Duration::from_nanos(dur_ns)
/// };
///
/// // Define a `Clock` implementation for the timestamp counter.
/// let clock = Clock::new(TICK_DURATION, || {
///     // A (pretend) function that reads the value of the timestamp
///     // counter. In real life, this might be a specific instruction,
///     // or a read from a timestamp counter register.
///     arch::read_timestamp_counter()
/// })
///     // Adding a name to the clock definition allows it to be i
///     // identified in fmt::Debug output.
///     .named("timestamp-counter");
///```
///
/// On some platforms, the frequency with which a timestamp counter is
/// incremented may be configured by setting a divisor that divides the base
/// frequency of the clock. On such a platform, it is possible to select the
/// tick duration when constructing a new `Clock`. We could then provide a
/// function that returns a clock with a requested tick duration:
///
///```rust
/// use maitake::time::{Clock, Duration};
/// # mod arch {
/// #     pub fn read_timestamp_counter() -> u64 { 0 }
/// #     pub fn set_clock_divisor(divisor: u32) {}
/// #     pub const CLOCK_BASE_DURATION_NS: u32 = 100;
/// # }
///
/// fn new_clock(tick_duration: Duration) -> Clock {
///     // Determine the divisor necessary to achieve the requested tick
///     // duration, based on the hardware clock's base frequency.
///     let divisor = {
///         let duration_ns = tick_duration.as_nanos();
///         assert!(
///             duration_ns as u32 >= arch::CLOCK_BASE_DURATION_NS,
///             "tick duration too short"
///         );
///         let div_u128 = duration_ns / arch::CLOCK_BASE_DURATION_NS as u128;
///         u32::try_from(div_u128).expect("tick duration too long")
///     };
///
///     // Set the divisor to the hardware clock. On real hardware, this
///     // might be a write to a register or memory-mapped IO location
///     // that controls the clock's divisor.
///     arch::set_clock_divisor(divisor as u32);
///
///     // Define a `Clock` implementation for the timestamp counter.
///     Clock::new(tick_duration, arch::read_timestamp_counter)
///         .named("timestamp-counter")
/// }
///```
///
/// In addition to timestamp-counter-based hardware clocks, a `Clock` definition
/// can be provided for an interrupt-based hardware clock. In this case, we
/// would provide an interrupt handler for the hardware timer interrupt that
/// increments an [`AtomicU64`](core::sync::atomic::AtomicU64) counter, and a
/// `now()` function that reads the counter. Essentially, we are reimplementing
/// a hardware timestamp counter in software, using the hardware timer interrupt
/// to increment the counter. For example:
///
/// ```rust
/// use maitake::time::{Clock, Duration};
/// use core::sync::atomic::{AtomicU64, Ordering};
/// # mod arch {
/// #    pub fn start_periodic_timer() -> u64 { 0 }
/// #    pub fn set_timer_period_ns(_: u64) {}
/// #    pub fn set_interrupt_handler(_: Interrupt, _: fn()) {}
/// #    pub enum Interrupt { Timer }
/// # }
///
/// // A counter that is incremented by the hardware timer interrupt.
/// static CLOCK_TICKS: AtomicU64 = AtomicU64::new(0);
///
/// // The hardware timer interrupt handler.
/// fn timer_interrupt_handler() {
///     // Increment the counter.
///     CLOCK_TICKS.fetch_add(1, Ordering::Relaxed);
/// }
///
/// // Returns the current timestamp by reading the counter.
/// fn now() -> u64 {
///     CLOCK_TICKS.load(Ordering::Relaxed)
/// }
///
/// fn new_clock(tick_duration: Duration) -> Clock {
///     // Set the hardware timer to generate periodic interrupts.
///     arch::set_timer_period_ns(tick_duration.as_nanos() as u64);
///
///     // Set the interrupt handler for the hardware timer interrupt,
///     // and start the timer.
///     arch::set_interrupt_handler(arch::Interrupt::Timer, timer_interrupt_handler);
///     arch::start_periodic_timer();
///
///    // Define a `Clock` implementation for the interrupt-based timer.
///    Clock::new(tick_duration, now)
///         .named("periodic-timer")
/// }
/// ```
/// [`std::time::Instant`]: https://doc.rust-lang.org/std/time/struct.Instant.html
/// [monotonic]: https://en.wikipedia.org/wiki/Monotonic_function
#[derive(Clone, Debug)]
pub struct Clock {
    now: fn() -> Ticks,
    tick_duration: Duration,
    name: &'static str,
}

/// A measurement of a monotonically nondecreasing [`Clock`].
/// Opaque and useful only with [`Duration`].
///
/// Provided that the [`Clock`] implementation is correct, `Instant`s are always
/// guaranteed to be no less than any previously measured instant when created,
/// and are often useful for tasks such as measuring benchmarks or timing how
/// long an operation takes.
///
/// Note, however, that instants are **not** guaranteed to be **steady**. In other
/// words, each tick of the underlying clock might not be the same length (e.g.
/// some seconds may be longer than others). An instant may jump forwards or
/// experience time dilation (slow down or speed up), but it will never go
/// backwards.
/// As part of this non-guarantee it is also not specified whether system suspends count as
/// elapsed time or not. The behavior varies across platforms and rust versions.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Instant(Duration);

/// [`Clock`] ticks are always counted by a 64-bit unsigned integer.
pub type Ticks = u64;

impl Clock {
    /// Returns a new [`Clock`] with the provided tick [`Duration`] and `now()`
    /// function.
    ///
    /// See the [type-level documentation for `Clock`](Self#implementing-now)
    /// for details on implementing the `now()` function.
    #[must_use]
    pub const fn new(tick_duration: Duration, now: fn() -> Ticks) -> Self {
        Self {
            now,
            tick_duration,
            name: "<unnamed mystery clock>",
        }
    }

    /// Add an arbitrary user-defined name to this `Clock`.
    ///
    /// This is generally used to describe the hardware time source used by the
    /// `now()` function for this `Clock`.
    #[must_use]
    pub const fn named(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    /// Returns the current `now` timestamp, in [`Ticks`] of this clock's base
    /// tick duration.
    #[must_use]
    pub(crate) fn now_ticks(&self) -> Ticks {
        (self.now)()
    }

    /// Returns the [`Duration`] of one tick of this clock.
    #[must_use]
    pub fn tick_duration(&self) -> Duration {
        self.tick_duration
    }

    /// Returns an [`Instant`] representing the current timestamp according to
    /// this [`Clock`].
    #[must_use]
    pub(crate) fn now(&self) -> Instant {
        let now = self.now_ticks();
        let tick_duration = self.tick_duration();
        Instant(ticks_to_dur(tick_duration, now))
    }

    /// Returns the maximum duration of this clock.
    #[must_use]
    pub fn max_duration(&self) -> Duration {
        max_duration(self.tick_duration())
    }

    /// Returns this `Clock`'s name, if it was given one using the [`Clock::named`]
    /// method.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }
}

#[track_caller]
#[inline]
#[must_use]
pub(in crate::time) fn ticks_to_dur(tick_duration: Duration, ticks: Ticks) -> Duration {
    const NANOS_PER_SEC: u32 = 1_000_000_000;
    // Multiply nanoseconds as u64, because it cannot overflow that way.
    let total_nanos = tick_duration.subsec_nanos() as u64 * ticks;
    let extra_secs = total_nanos / (NANOS_PER_SEC as u64);
    let nanos = (total_nanos % (NANOS_PER_SEC as u64)) as u32;
    let Some(secs) = tick_duration.as_secs().checked_mul(ticks) else {
        panic!(
            "ticks_to_dur({tick_duration:?}, {ticks}): multiplying tick \
            duration seconds by ticks would overflow"
        );
    };
    let Some(secs) = secs.checked_add(extra_secs) else {
        panic!("ticks_to_dur({tick_duration:?}, {ticks}): extra seconds from nanos ({extra_secs}s) would overflow total seconds")
    };
    debug_assert!(nanos < NANOS_PER_SEC);
    Duration::new(secs, nanos)
}

#[track_caller]
#[inline]
pub(in crate::time) fn dur_to_ticks(
    tick_duration: Duration,
    dur: Duration,
) -> Result<Ticks, TimerError> {
    (dur.as_nanos() / tick_duration.as_nanos())
        .try_into()
        .map_err(|_| TimerError::DurationTooLong {
            requested: dur,
            max: max_duration(tick_duration),
        })
}

#[track_caller]
#[inline]
#[must_use]
pub(in crate::time) fn max_duration(tick_duration: Duration) -> Duration {
    tick_duration.saturating_mul(u32::MAX)
}

impl Instant {
    /// Returns an instant corresponding to "now".
    ///
    /// This function uses the [global default timer][global]. See [the
    /// module-level documentation][global] for details on using the global
    /// default timer.
    ///
    /// # Panics
    ///
    /// This function panics if the [global default timer][global] has not been
    /// set.
    ///
    /// For a version of this function that returns a [`Result`] rather than
    /// panicking, use [`Instant::try_now`] instead.
    ///
    /// [global]: crate::time#global-timers
    #[must_use]
    pub fn now() -> Instant {
        Self::try_now().expect("no global timer set")
    }

    /// Returns an instant corresponding to "now", without panicking.
    ///
    /// This function uses the [global default timer][global]. See [the
    /// module-level documentation][global] for details on using the global
    /// default timer.
    ///
    /// # Returns
    ///
    /// - [`Ok`]`(`[`Instant`]`)` if the [global default timer] is available.
    /// - [`Err`]`(`[`TimerError`]`)` if no [global default timer][global] has
    ///   been set.
    ///
    /// [global]: crate::time#global-timers
    pub fn try_now() -> Result<Self, TimerError> {
        Ok(timer::global::default()?.now())
    }

    /// Returns the amount of time elapsed from another instant to this one,
    /// or zero duration if that instant is later than this one.
    #[must_use]
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.checked_duration_since(earlier).unwrap_or_default()
    }

    /// Returns the amount of time elapsed from another instant to this one,
    /// or [`None`]` if that instant is later than this one.
    #[must_use]
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.0.checked_sub(earlier.0)
    }

    /// Returns the amount of time elapsed since this instant.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.0
    }

    /// Returns `Some(t)` where `t` is the time `self + duration` if `t` can be represented as
    /// `Instant` (which means it's inside the bounds of the underlying data structure), [`None`]
    /// otherwise.
    #[must_use]
    pub fn checked_add(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_add(duration).map(Instant)
    }

    /// Returns `Some(t)` where `t` is the time `self - duration` if `t` can be represented as
    /// `Instant` (which means it's inside the bounds of the underlying data structure), [`None`]
    /// otherwise.
    #[must_use]
    pub fn checked_sub(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_sub(duration).map(Instant)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;

    /// # Panics
    ///
    /// This function may panic if the resulting point in time cannot be represented by the
    /// underlying data structure. See [`Instant::checked_add`] for a version without panic.
    fn add(self, other: Duration) -> Instant {
        self.checked_add(other)
            .expect("overflow when adding duration to instant")
    }
}

impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, other: Duration) {
        *self = *self + other;
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;

    fn sub(self, other: Duration) -> Instant {
        self.checked_sub(other)
            .expect("overflow when subtracting duration from instant")
    }
}

impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, other: Duration) {
        *self = *self - other;
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;

    /// Returns the amount of time elapsed from another instant to this one,
    /// or zero duration if that instant is later than this one.
    fn sub(self, other: Instant) -> Duration {
        self.duration_since(other)
    }
}

impl fmt::Display for Instant {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
