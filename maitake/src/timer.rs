//! Timer utilities.
use crate::loom::{
    atomic::{AtomicUsize, Ordering::*},
    sync::spin::Mutex,
};
use core::pin::Pin;
mod sleep;
mod wheel;

pub use self::sleep::Sleep;
use self::wheel::Wheel;

pub type Ticks = u64;

pub struct Timer {
    pending_ticks: AtomicUsize,
    core: Mutex<Core>,
}

struct Core {
    // ... this will be the actual timer wheel ...
    wheels: [Wheel; 6],
}

impl Timer {
    loom_const_fn! {
        pub fn new() -> Self {
            Self {
                pending_ticks: AtomicUsize::new(0),
                core: Mutex::new(Core {
                    wheels: [
                        Wheel::new(0),
                        Wheel::new(1),
                        Wheel::new(2),
                        Wheel::new(3),
                        Wheel::new(4),
                        Wheel::new(5),
                    ],
                }),
            }
        }
    }

    /// Returns a future that will complete in `ticks` timer ticks.
    pub fn sleep(&self, ticks: Ticks) -> Sleep<'_> {
        todo!("eliza")
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
    pub const fn new() -> Self {
        Self {
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
        todo!("actually advance the timer wheel")
    }

    fn cancel_sleep(&mut self, sleep: Pin<&mut sleep::Entry>) {
        todo!("cancel a sleep")
    }
}
