use crate::loom::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicUsize,
        Ordering::{self, *},
    },
};
use core::{
    future::Future,
    ops,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use mycelium_util::{fmt, sync::CachePadded};

/// An error indicating that a [`WaitCell`] was closed or busy while
/// attempting register a waiter.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Closed,
    Notifying,
    Parking,
}

const fn closed() -> Result<(), Error> {
    Err(Error::Closed)
}

const fn parking() -> Result<(), Error> {
    Err(Error::Parking)
}

const fn registered() -> Result<(), Error> {
    Ok(())
}

const fn notifying() -> Result<(), Error> {
    Err(Error::Notifying)
}

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

/// Future returned from [`WaitCell::wait()`].
///
/// This future is fused, so once it has completed, any future calls to poll
/// will immediately return [`Poll::Ready`].
#[derive(Debug)]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Wait<'a> {
    /// The [`WaitQueue`] being waited on from.
    cell: &'a WaitCell,

    /// Whether we have already polled once
    registered: bool,
}

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
    pub fn register_wait(&self, waker: &Waker) -> Result<(), Error> {
        trace!(wait_cell = ?fmt::ptr(self), ?waker, "registering waker");

        // this is based on tokio's AtomicWaker synchronization strategy
        match test_dbg!(self.compare_exchange(State::WAITING, State::PARKING, Acquire)) {
            // someone else is notifying, so don't wait!
            Err(actual) if test_dbg!(actual.is(State::CLOSED)) => {
                return closed();
            }
            Err(actual) if test_dbg!(actual.is(State::NOTIFYING)) => {
                return notifying();
            }

            Err(actual) => {
                debug_assert!(
                    actual == State::PARKING || actual == State::PARKING | State::NOTIFYING
                );
                return parking();
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

        if let Some(prev_waker) = prev_waker {
            test_trace!("Replaced an old waker in cell, waking");
            prev_waker.wake();
        }

        match test_dbg!(self.compare_exchange(State::PARKING, State::WAITING, AcqRel)) {
            Ok(_) => registered(),
            Err(actual) => {
                // If the `compare_exchange` fails above, this means that we were notified for one of
                // two reasons: either the cell was awoken, or the cell was closed.
                //
                // Bail out of the parking state, and determine what to report to the caller.
                test_trace!(state = ?actual, "was notified");
                let waker = self.waker.with_mut(|waker| unsafe { (*waker).take() });
                // Reset to the WAITING state by clearing everything *except*
                // the closed bits (which must remain set). This `fetch_and`
                // does *not* set the CLOSED bit if it is unset, it just doesn't
                // clear it.
                let state = test_dbg!(self.fetch_and(State::CLOSED, AcqRel));
                // The only valid state transition while we were parking is to
                // add the CLOSED bit.
                debug_assert!(
                    state == actual || state == actual | State::CLOSED,
                    "state changed unexpectedly while parking!"
                );

                if let Some(waker) = waker {
                    waker.wake();
                }

                // Was the `CLOSED` bit set while we were clearing other bits?
                // If so, the cell is closed. Otherwise, we must have been notified.
                if state.is(State::CLOSED) {
                    closed()
                } else {
                    notifying()
                }
            }
        }
    }

    /// Wait to be woken up by this cell.
    ///
    /// Note: The waiter is not registered until AFTER the first time the waiter is polled
    pub fn wait(&self) -> Wait<'_> {
        Wait {
            cell: self,
            registered: false,
        }
    }

    pub fn wake(&self) -> bool {
        self.notify2(State::WAITING)
    }

    pub fn close(&self) {
        self.notify2(State::CLOSED);
    }

    fn notify2(&self, close: State) -> bool {
        trace!(wait_cell = ?fmt::ptr(self), ?close, "notifying");
        let bits = State::NOTIFYING | close;
        if test_dbg!(self.fetch_or(bits, AcqRel)) == State::WAITING {
            // we have the lock!
            let waker = self.waker.with_mut(|thread| unsafe { (*thread).take() });

            test_dbg!(self.fetch_and(!State::NOTIFYING, AcqRel));

            if let Some(waker) = test_dbg!(waker) {
                trace!(wait_cell = ?fmt::ptr(self), ?close, ?waker, "notified");
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

impl Drop for WaitCell {
    fn drop(&mut self) {
        self.close();
    }
}

// === impl Wait ===

impl Future for Wait<'_> {
    type Output = Result<(), super::Closed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.registered {
            // We made it to "once", and got polled again, we must be ready!
            return Poll::Ready(Ok(()));
        }

        match test_dbg!(self.cell.register_wait(cx.waker())) {
            Ok(_) => {
                self.registered = true;
                Poll::Pending
            }
            Err(Error::Parking) => {
                // Cell was busy parking some other task, all we can do is try again later
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(Error::Notifying) => {
                // Cell is waking another task RIGHT NOW, so let's ride that high all the
                // way to the READY state.
                Poll::Ready(Ok(()))
            }
            Err(Error::Closed) => super::closed(),
        }
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

#[cfg(all(feature = "alloc", not(loom), test))]
mod tests {
    use super::*;
    use crate::scheduler::Scheduler;
    use alloc::sync::Arc;

    #[test]
    fn wait_smoke() {
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        let sched = Scheduler::new();
        let wait = Arc::new(WaitCell::new());

        let wait2 = wait.clone();
        sched.spawn(async move {
            wait2.wait().await.unwrap();
            COMPLETED.fetch_add(1, Ordering::Relaxed);
        });

        let tick = sched.tick();
        assert_eq!(tick.completed, 0);
        assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);

        assert_eq!(wait.wake(), true);
        let tick = sched.tick();
        assert_eq!(tick.completed, 1);
        assert_eq!(COMPLETED.load(Ordering::Relaxed), 1);
    }
}

#[cfg(test)]
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

                let res = this.task.wait();
                futures_util::pin_mut!(res);

                if this.num_notify == this.num.load(Relaxed) {
                    return Poll::Ready(());
                }

                res.poll(cx).map(drop)
            })
            .await
        }

        pub(crate) fn wake(&self) {
            self.num.fetch_add(1, Relaxed);
            self.task.wake();
        }

        #[allow(dead_code)]
        pub(crate) fn close(&self) {
            self.num.fetch_add(1, Relaxed);
            self.task.close();
        }
    }

    impl Drop for Chan {
        fn drop(&mut self) {
            debug!(chan = ?fmt::alt(&self), "drop");
        }
    }
}

#[cfg(all(loom, test))]
mod loom {
    use super::*;
    use crate::loom::{future, sync::Arc, thread};

    #[test]
    fn basic() {
        crate::loom::model(|| {
            let wait = Arc::new(WaitCell::new());

            let waker = wait.clone();
            let closer = wait.clone();

            thread::spawn(move || {
                info!("waking");
                waker.wake();
                info!("woken");
            });
            thread::spawn(move || {
                info!("closing");
                closer.close();
                info!("closed");
            });

            info!("waiting");
            let _ = future::block_on(wait.wait());
            info!("wait'd");
        });
    }
}
