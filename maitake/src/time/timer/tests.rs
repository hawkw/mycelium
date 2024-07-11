#[cfg(any(loom, feature = "alloc"))]
use super::*;

use core::cell::RefCell;
use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::time::{Clock, timer::Ticks};
use std::time::Duration;

crate::loom::thread_local! {
    static CLOCK: RefCell<Option<Arc<TestClockState>>> = RefCell::new(None);
}

#[derive(Debug, Clone)]
pub struct TestClock(Arc<TestClockState>);

#[derive(Debug)]
struct TestClockState {
    now: AtomicU64,
}

#[derive(Debug)]
#[must_use = "dropping a test clock immediately resets the clock"]
pub struct ClockHandle(Arc<TestClockState>);

impl TestClock {
    pub fn start() -> ClockHandle {
        let this = Self(Arc::new(TestClockState {
            now: AtomicU64::new(0),
        }));
        this.enter()
    }

    pub fn enter(&self) -> ClockHandle {
        CLOCK.with(|current| {
            let prev = current.replace(Some(self.0.clone()));
            assert!(prev.is_none(), "test clock already started!");
        });
        ClockHandle(self.0.clone())
    }

    pub const fn clock() -> Clock {
        Clock::new(Duration::from_millis(1), Self::now)
    }

    fn now() -> u64 {
        CLOCK.with(|current| {
            let current = current.borrow();
            let clock = current
                .as_ref()
                .expect("the test clock has not started on this thread!");
            clock.now.load(Ordering::SeqCst)
        })
    }
}

impl ClockHandle {
    pub fn advance(&self, duration: Duration) {
        info!(?duration, "advancing test clock by");
        self.advance_ticks_inner(duration.as_millis() as u64)
    }

    // This function isn't used in loom tests.
    #[cfg_attr(loom, allow(dead_code))]
    pub fn advance_ticks(&self, ticks: Ticks) {
        info!(ticks, "advancing test clock by");
        self.advance_ticks_inner(ticks)
    }

    fn advance_ticks_inner(&self, ticks: Ticks) {
        let prev = self.0.now.fetch_add(ticks, Ordering::SeqCst);
        assert!(
            Ticks::MAX - prev >= ticks,
            "clock overflowed (now: {prev}; advanced by {ticks})"
        );
    }

    // This function isn't used in loom tests.
    #[cfg_attr(loom, allow(dead_code))]
    pub fn ticks(&self) -> Ticks {
        self.0.now.load(Ordering::SeqCst)
    }

    pub fn test_clock(&self) -> TestClock {
        TestClock(self.0.clone())
    }
}

impl Drop for ClockHandle {
    fn drop(&mut self) {
        let _ = CLOCK.try_with(|current| current.borrow_mut().take());
    }
}

#[cfg(all(feature = "alloc", not(loom)))]
mod wheel_tests;

#[cfg(feature = "alloc")]
mod concurrent;
