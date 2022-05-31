use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{
            AtomicUsize,
            Ordering::{self, *},
        },
    },
    util::tracing,
    wait::{self, WaitResult},
};
use core::{
    ops,
    task::{Poll, Waker},
};
use mycelium_util::{fmt, sync::CachePadded};

/// An atomically registered [`Waker`].
///
/// This is inspired by the [`AtomicWaker` type] used in Tokio's
/// synchronization primitives, with the following modifications:
///
/// - An additional bit of state is added to allow setting a "close" bit.
/// - A `WaitCell` is always woken by value (for now).
/// - `WaitCell` does not handle unwinding, because mycelium kernel-mode code
///   does not support unwinding.
///
/// [`AtomicWaker`]: https://github.com/tokio-rs/tokio/blob/09b770c5db31a1f35631600e1d239679354da2dd/tokio/src/sync/task/atomic_waker.rs
/// [`Waker`]: core::task::Waker
pub struct WaitCell {
    lock: CachePadded<AtomicUsize>,
    waker: UnsafeCell<Option<Waker>>,
}

#[derive(Eq, PartialEq, Copy, Clone)]
struct State(usize);

// === impl WaitCell ===

impl WaitCell {
    #[cfg(not(all(loom, test)))]
    pub const fn new() -> Self {
        Self {
            lock: CachePadded::new(AtomicUsize::new(State::WAITING.0)),
            waker: UnsafeCell::new(None),
        }
    }

    #[cfg(all(loom, test))]
    pub fn new() -> Self {
        Self {
            lock: CachePadded::new(AtomicUsize::new(State::WAITING.0)),
            waker: UnsafeCell::new(None),
        }
    }
}

impl WaitCell {
    pub fn poll_wait(&self, waker: &Waker) -> Poll<WaitResult> {
        tracing::trace!(wait_cell = ?fmt::ptr(self), ?waker, "registering waker");

        // this is based on tokio's AtomicWaker synchronization strategy
        match test_dbg!(self.compare_exchange(State::WAITING, State::PARKING, Acquire)) {
            // someone else is notifying, so don't wait!
            Err(actual) if test_dbg!(actual.is(State::CLOSED)) => {
                return wait::closed();
            }
            Err(actual) if test_dbg!(actual.is(State::NOTIFYING)) => {
                waker.wake_by_ref();
                crate::loom::hint::spin_loop();
                return wait::notified();
            }

            Err(actual) => {
                debug_assert!(
                    actual == State::PARKING || actual == State::PARKING | State::NOTIFYING
                );
                return Poll::Pending;
            }
            Ok(_) => {}
        }

        test_trace!("-> wait cell locked!");
        let prev_waker = self.waker.with_mut(|old_waker| unsafe {
            match &mut *old_waker {
                Some(old_waker) if waker.will_wake(old_waker) => None,
                old => old.replace(waker.clone()),
            }
        });

        match test_dbg!(self.compare_exchange(State::PARKING, State::WAITING, AcqRel)) {
            Ok(_) => Poll::Pending,
            Err(actual) => {
                test_trace!(state = ?actual, "was notified");
                let waker = self.waker.with_mut(|waker| unsafe { (*waker).take() });
                // Reset to the WAITING state by clearing everything *except*
                // the closed bits (which must remain set).
                let state = test_dbg!(self.fetch_and(State::CLOSED, AcqRel));
                // The only valid state transition while we were parking is to
                // add the CLOSED bit.
                debug_assert!(
                    state == actual || state == actual | State::CLOSED,
                    "state changed unexpectedly while parking!"
                );

                if let Some(prev_waker) = prev_waker {
                    prev_waker.wake();
                }

                if let Some(waker) = waker {
                    waker.wake();
                }

                if test_dbg!(state.is(State::CLOSED)) {
                    wait::closed()
                } else {
                    wait::notified()
                }
            }
        }
    }

    pub fn notify(&self) -> bool {
        self.notify2(State::WAITING)
    }

    pub fn close(&self) {
        self.notify2(State::CLOSED);
    }

    fn notify2(&self, close: State) -> bool {
        tracing::trace!(wait_cell = ?fmt::ptr(self), ?close, "notifying");
        let bits = State::NOTIFYING | close;
        if test_dbg!(self.fetch_or(bits, AcqRel)) == State::WAITING {
            // we have the lock!
            let waker = self.waker.with_mut(|thread| unsafe { (*thread).take() });

            test_dbg!(self.fetch_and(!State::NOTIFYING, AcqRel));

            if let Some(waker) = test_dbg!(waker) {
                tracing::trace!(wait_cell = ?fmt::ptr(self), ?close, ?waker, "notified");
                waker.wake();
                return true;
            }
        }
        false
    }
}

impl WaitCell {
    #[inline(always)]
    fn compare_exchange(
        &self,
        State(curr): State,
        State(new): State,
        success: Ordering,
    ) -> Result<State, State> {
        self.lock
            .compare_exchange(curr, new, success, Acquire)
            .map(State)
            .map_err(State)
    }

    #[inline(always)]
    fn fetch_and(&self, State(state): State, order: Ordering) -> State {
        State(self.lock.fetch_and(state, order))
    }

    #[inline(always)]
    fn fetch_or(&self, State(state): State, order: Ordering) -> State {
        State(self.lock.fetch_or(state, order))
    }

    #[inline(always)]
    fn current_state(&self) -> State {
        State(self.lock.load(Acquire))
    }
}

unsafe impl Send for WaitCell {}
unsafe impl Sync for WaitCell {}

impl fmt::Debug for WaitCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WaitCell")
            .field("state", &self.current_state())
            .field("waker", &fmt::display(".."))
            .finish()
    }
}

// === impl State ===

impl State {
    const WAITING: Self = Self(0b00);
    const PARKING: Self = Self(0b01);
    const NOTIFYING: Self = Self(0b10);
    const CLOSED: Self = Self(0b100);

    fn is(self, Self(state): Self) -> bool {
        self.0 & state == state
    }
}

impl ops::BitOr for State {
    type Output = Self;

    fn bitor(self, Self(rhs): Self) -> Self::Output {
        Self(self.0 | rhs)
    }
}

impl ops::Not for State {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut has_states = false;

        fmt_bits!(self, f, has_states, PARKING, NOTIFYING, CLOSED);

        if !has_states {
            if *self == Self::WAITING {
                return f.write_str("WAITING");
            }

            f.debug_tuple("UnknownState")
                .field(&format_args!("{:#b}", self.0))
                .finish()?;
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod test_util {
    use super::*;

    use crate::loom::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    use std::sync::Arc;

    #[derive(Debug)]
    pub(crate) struct Chan {
        num: AtomicUsize,
        task: WaitCell,
        num_notify: usize,
    }

    impl Chan {
        pub(crate) fn new(num_notify: usize) -> Arc<Self> {
            Arc::new(Self {
                num: AtomicUsize::new(0),
                task: WaitCell::new(),
                num_notify,
            })
        }

        pub(crate) async fn wait(self: Arc<Chan>) {
            let this = Arc::downgrade(&self);
            drop(self);
            futures_util::future::poll_fn(move |cx| {
                let this = match this.upgrade() {
                    Some(this) => this,
                    None => return Poll::Ready(()),
                };

                let res = test_dbg!(this.task.poll_wait(cx.waker()));

                if this.num_notify == this.num.load(Relaxed) {
                    return Poll::Ready(());
                }

                if res.is_ready() {
                    return Poll::Ready(());
                }

                Poll::Pending
            })
            .await
        }

        pub(crate) fn notify(&self) {
            self.num.fetch_add(1, Relaxed);
            self.task.notify();
        }

        pub(crate) fn close(&self) {
            self.num.fetch_add(1, Relaxed);
            self.task.close();
        }
    }

    impl Drop for Chan {
        fn drop(&mut self) {
            tracing::debug!(chan = ?fmt::alt(self), "drop")
        }
    }
}

#[cfg(all(loom, test))]
mod loom {
    use super::*;
    use crate::loom::{future, thread};

    const NUM_NOTIFY: usize = 2;

    #[test]
    fn basic_latch() {
        crate::loom::model(|| {
            let chan = test_util::Chan::new(NUM_NOTIFY);

            for _ in 0..NUM_NOTIFY {
                let chan = chan.clone();

                thread::spawn(move || chan.notify());
            }

            future::block_on(chan.wait());
        });
    }

    #[test]
    fn close() {
        crate::loom::model(|| {
            let chan = test_util::Chan::new(NUM_NOTIFY);

            thread::spawn({
                let chan = chan.clone();
                move || {
                    chan.notify();
                }
            });

            thread::spawn({
                let chan = chan.clone();
                move || {
                    chan.close();
                }
            });

            future::block_on(chan.wait());
        });
    }
}
