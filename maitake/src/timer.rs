//! Timer utilities.
use crate::loom::sync::{
    atomic::{AtomicUsize, Ordering::*},
    spin::Mutex,
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

    /// Advance the timer by `ticks`, waking any `Sleep` futures that have
    /// completed.
    #[inline]
    pub fn advance(&self, ticks: Ticks) {
        // `advance` may be called in an ISR, so it can never actually spin.
        // instead, if the timer wheel is busy (e.g. the timer ISR was called on
        // another core, or if a `Sleep` future is currently canceling itself),
        // we just add to a counter of pending ticks, and bail.
        if let Some(mut core) = self.core.try_lock() {
            // take any pending ticks.
            let pending_ticks = self.pending_ticks.swap(0, AcqRel) as Ticks;
            // we do two separate `advance` calls here instead of advancing once
            // with the sum, because `ticks` + `pending_ticks` could overflow.
            if pending_ticks > 0 {
                core.advance(pending_ticks);
            }
            core.advance(ticks);
        } else {
            // if the core of the timer wheel is already locked, add to the pending
            // tick count, which we will then advance the wheel by when it becomes
            // available.
            // TODO(eliza): if pending ticks overflows that's probably Bad News
            self.pending_ticks.fetch_add(ticks as usize, Release);
        }
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
