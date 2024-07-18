//! An atomically registered [`Waker`], for waking a single task.
//!
//! See the documentation for the [`WaitCell`] type for details.
use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{
            AtomicUsize,
            Ordering::{self, *},
        },
    },
    util::{fmt, CachePadded},
    Closed,
};
use core::{
    future::Future,
    ops,
    pin::Pin,
    task::{self, Context, Poll, Waker},
};

/// An atomically registered [`Waker`].
///
/// This cell stores the [`Waker`] of a single task. A [`Waker`] is stored in
/// the cell either by calling [`poll_wait`], or by polling a [`wait`]
/// future. Once a task's [`Waker`] is stored in a `WaitCell`, it can be woken
/// by calling [`wake`] on the `WaitCell`.
///
/// # Implementation Notes
///
/// This is inspired by the [`AtomicWaker`] type used in Tokio's
/// synchronization primitives, with the following modifications:
///
/// - An additional bit of state is added to allow [setting a "close"
///   bit](Self::close).
/// - A `WaitCell` is always woken by value (for now).
/// - `WaitCell` does not handle unwinding, because [`maitake` does not support
///   unwinding](crate#maitake-does-not-support-unwinding)
///
/// [`AtomicWaker`]: https://github.com/tokio-rs/tokio/blob/09b770c5db31a1f35631600e1d239679354da2dd/tokio/src/sync/task/atomic_waker.rs
/// [`Waker`]: core::task::Waker
/// [`poll_wait`]: Self::poll_wait
/// [`wait`]: Self::wait
/// [`wake`]: Self::wake
pub struct WaitCell {
    state: CachePadded<AtomicUsize>,
    waker: UnsafeCell<Option<Waker>>,
}

/// An error indicating that a [`WaitCell`] was closed or busy while
/// attempting register a [`Waker`].
///
/// This error is returned by the [`WaitCell::poll_wait`] method.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PollWaitError {
    /// The [`Waker`] was not registered because the [`WaitCell`] has been
    /// [closed](WaitCell::close).
    Closed,

    /// The [`Waker`] was not registered because another task was concurrently
    /// storing its own [`Waker`] in the [`WaitCell`].
    Busy,
}

/// Future returned from [`WaitCell::wait()`].
///
/// This future is fused, so once it has completed, any future calls to poll
/// will immediately return [`Poll::Ready`].
#[derive(Debug)]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Wait<'a> {
    /// The [`WaitCell`] being waited on.
    cell: &'a WaitCell,

    presubscribe: Poll<Result<(), super::Closed>>,
}

/// Future returned from [`WaitCell::subscribe()`].
///
/// See the documentation for [`WaitCell::subscribe()`] for details.
#[derive(Debug)]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Subscribe<'a> {
    /// The [`WaitCell`] being waited on.
    cell: &'a WaitCell,
}

#[derive(Eq, PartialEq, Copy, Clone)]
struct State(usize);

// === impl WaitCell ===

impl WaitCell {
    loom_const_fn! {
        /// Returns a new `WaitCell`, with no [`Waker`] stored in it.
        #[must_use]
        pub fn new() -> Self {
            Self {
                state: CachePadded::new(AtomicUsize::new(State::WAITING.0)),
                waker: UnsafeCell::new(None),
            }
        }
    }
}

impl WaitCell {
    /// Poll to wait on this `WaitCell`, consuming a stored wakeup or
    /// registering the [`Waker`] from the provided [`Context`] to be woken by
    /// the next wakeup.
    ///
    /// Once a [`Waker`] has been registered, a subsequent call to [`wake`] will
    /// wake that [`Waker`].
    ///
    /// # Returns
    ///
    /// - [`Poll::Pending`] if the [`Waker`] was registered. If this method returns
    ///   [`Poll::Pending`], then the registered [`Waker`] will be woken by a
    ///   subsequent call to [`wake`].
    /// - [`Poll::Ready`]`(`[`Ok`]`(()))` if the cell was woken by a call to
    ///   [`wake`] while the [`Waker`] was being registered.
    /// - [`Poll::Ready`]`(`[`Err`]`(`[`PollWaitError::Closed`]`))` if the
    ///   [`WaitCell`] has been closed.
    /// - [`Poll::Ready`]`(`[`Err`]`(`[`PollWaitError::Busy`]`))` if another
    ///   task was concurrently registering its [`Waker`] with this
    ///   [`WaitCell`].
    ///
    /// [`wake`]: Self::wake
    pub fn poll_wait(&self, cx: &mut Context<'_>) -> Poll<Result<(), PollWaitError>> {
        enter_test_debug_span!("WaitCell::poll_wait", cell = ?fmt::ptr(self));

        // this is based on tokio's AtomicWaker synchronization strategy
        match test_dbg!(self.compare_exchange(State::WAITING, State::REGISTERING, Acquire)) {
            Err(actual) if test_dbg!(actual.contains(State::CLOSED)) => {
                return Poll::Ready(Err(PollWaitError::Closed));
            }
            Err(actual) if test_dbg!(actual.contains(State::WOKEN)) => {
                // take the wakeup
                self.fetch_and(!State::WOKEN, Release);
                return Poll::Ready(Ok(()));
            }
            // someone else is notifying, so don't wait!
            Err(actual) if test_dbg!(actual.contains(State::WAKING)) => {
                return Poll::Ready(Ok(()));
            }
            Err(_) => return Poll::Ready(Err(PollWaitError::Busy)),
            Ok(_) => {}
        }

        let waker = cx.waker();
        trace!(wait_cell = ?fmt::ptr(self), ?waker, "registering waker");

        let prev_waker = self.waker.with_mut(|old_waker| unsafe {
            match &mut *old_waker {
                Some(old_waker) if waker.will_wake(old_waker) => None,
                old => old.replace(waker.clone()),
            }
        });

        if let Some(prev_waker) = prev_waker {
            test_debug!("Replaced an old waker in cell, waking");
            prev_waker.wake();
        }

        if let Err(actual) =
            test_dbg!(self.compare_exchange(State::REGISTERING, State::WAITING, AcqRel))
        {
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
            if state.contains(State::CLOSED) {
                return Poll::Ready(Err(PollWaitError::Closed));
            }

            return Poll::Ready(Ok(()));
        }

        // Waker registered, time to yield!
        Poll::Pending
    }

    /// Wait to be woken up by this cell.
    ///
    /// # Returns
    ///
    /// This future completes with the following values:
    ///
    /// - [`Ok`]`(())` if the future was woken by a call to [`wake`] or another
    ///   task calling [`poll_wait`] or [`wait`] on this [`WaitCell`].
    /// - [`Err`]`(`[`Closed`]`)` if the task was woken by a call to [`close`],
    ///   or the [`WaitCell`] was already closed.
    ///
    /// **Note**: The calling task's [`Waker`] is not registered until AFTER the
    /// first time the returned [`Wait`] future is polled. This means that if a
    /// call to [`wake`] occurs between when [`wait`] is called and when the
    /// future is first polled, the future will *not* complete. If the caller is
    /// responsible for performing an operation which will result in an eventual
    /// wakeup, prefer calling [`subscribe`] _before_ performing that operation
    /// and `.await`ing the [`Wait`] future returned by [`subscribe`].
    ///
    /// [`wake`]: Self::wake
    /// [`poll_wait`]: Self::poll_wait
    /// [`wait`]: Self::wait
    /// [`close`]: Self::close
    /// [`subscribe`]: Self::subscribe
    pub fn wait(&self) -> Wait<'_> {
        Wait {
            cell: self,
            presubscribe: Poll::Pending,
        }
    }

    /// Eagerly subscribe to notifications from this `WaitCell`.
    ///
    /// This method returns a [`Subscribe`] [`Future`], which outputs a [`Wait`]
    /// [`Future`]. Awaiting the [`Subscribe`] future will eagerly register the
    /// calling task to be woken by this [`WaitCell`], so that the returned
    /// [`Wait`] future will be woken by any calls to [`wake`] (or [`close`])
    /// that occur between when the [`Subscribe`] future completes and when the
    /// returned [`Wait`] future is `.await`ed.
    ///
    /// This is primarily intended for scenarios where the task that waits on a
    /// [`WaitCell`] is responsible for performing some operation that
    /// ultimately results in the [`WaitCell`] being woken. If the task were to
    /// simply perform the operation and then call [`wait`] on the [`WaitCell`],
    /// a potential race condition could occur where the operation completes and
    /// wakes the [`WaitCell`] *before* the [`Wait`] future is first `.await`ed.
    /// Using `subscribe`, the task can ensure that it is ready to be woken by
    /// the cell *before* performing an operation that could result in it being
    /// woken.
    ///
    /// These scenarios occur when a wakeup is triggered by another thread/CPU
    /// core in response to an operation performed in the task waiting on the
    /// `WaitCell`, or when the wakeup is triggered by a hardware interrupt
    /// resulting from operations performed in the task.
    ///
    /// # Examples
    ///
    /// ```
    /// use maitake_sync::WaitCell;
    ///
    /// // Perform an operation that results in a concurrent wakeup, such as
    /// // unmasking an interrupt.
    /// fn do_something_that_causes_a_wakeup() {
    ///     # WAIT_CELL.wake();
    ///     // ...
    /// }
    ///
    /// static WAIT_CELL: WaitCell = WaitCell::new();
    ///
    /// # async fn dox() {
    /// // Subscribe to notifications from the cell *before* calling
    /// // `do_something_that_causes_a_wakeup()`, to ensure that we are
    /// // ready to be woken when the interrupt is unmasked.
    /// let wait = WAIT_CELL.subscribe().await;
    ///
    /// // Actually perform the operation.
    /// do_something_that_causes_a_wakeup();
    ///
    /// // Wait for the wakeup. If the wakeup occurred *before* the first
    /// // poll of the `wait` future had successfully subscribed to the
    /// // `WaitCell`, we would still receive the wakeup, because the
    /// // `subscribe` future ensured that our waker was registered to be
    /// // woken.
    /// wait.await.expect("WaitCell is not closed");
    /// # }
    /// ```
    ///
    /// [`wait`]: Self::wait
    /// [`wake`]: Self::wake
    /// [`close`]: Self::close
    pub fn subscribe(&self) -> Subscribe<'_> {
        Subscribe { cell: self }
    }

    /// Wake the [`Waker`] stored in this cell.
    ///
    /// # Returns
    ///
    /// - `true` if a waiting task was woken.
    /// - `false` if no task was woken (no [`Waker`] was stored in the cell)
    pub fn wake(&self) -> bool {
        enter_test_debug_span!("WaitCell::wake", cell = ?fmt::ptr(self));
        if let Some(waker) = self.take_waker(false) {
            waker.wake();
            true
        } else {
            false
        }
    }

    /// Close the [`WaitCell`].
    ///
    /// This wakes any waiting task with an error indicating the `WaitCell` is
    /// closed. Subsequent calls to [`wait`] or [`poll_wait`] will return an
    /// error indicating that the cell has been closed.
    ///
    /// [`wait`]: Self::wait
    /// [`poll_wait`]: Self::poll_wait
    pub fn close(&self) -> bool {
        enter_test_debug_span!("WaitCell::close", cell = ?fmt::ptr(self));
        if let Some(waker) = self.take_waker(true) {
            waker.wake();
            true
        } else {
            false
        }
    }

    /// Asynchronously poll the given function `f` until a condition occurs,
    /// using the [`WaitCell`] to only re-poll when notified.
    ///
    /// This can be used to implement a "wait loop", turning a "try" function
    /// (e.g. "try_recv" or "try_send") into an asynchronous function (e.g.
    /// "recv" or "send").
    ///
    /// In particular, this function correctly *registers* interest in the [`WaitCell`]
    /// prior to polling the function, ensuring that there is not a chance of a race
    /// where the condition occurs AFTER checking but BEFORE registering interest
    /// in the [`WaitCell`], which could lead to deadlock.
    ///
    /// This is intended to have similar behavior to `Condvar` in the standard library,
    /// but asynchronous, and not requiring operating system intervention (or existence).
    ///
    /// In particular, this can be used in cases where interrupts or events are used
    /// to signify readiness or completion of some task, such as the completion of a
    /// DMA transfer, or reception of an ethernet frame. In cases like this, the interrupt
    /// can wake the cell, allowing the polling function to check status fields for
    /// partial progress or completion.
    ///
    /// Consider using [`Self::wait_for_value()`] if your function does return a value.
    ///
    /// Consider using [`WaitQueue::wait_for()`](super::wait_queue::WaitQueue::wait_for)
    /// if you need multiple waiters.
    ///
    /// # Returns
    ///
    /// * [`Ok`]`(())` if the closure returns `true`.
    /// * [`Err`]`(`[`Closed`]`)` if the [`WaitCell`] is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// use std::sync::Arc;
    /// use maitake_sync::WaitCell;
    /// use std::sync::atomic::{AtomicU8, Ordering};
    ///
    /// let queue = Arc::new(WaitCell::new());
    /// let num = Arc::new(AtomicU8::new(0));
    ///
    /// let waiter = task::spawn({
    ///     // clone items to move into the spawned task
    ///     let queue = queue.clone();
    ///     let num = num.clone();
    ///     async move {
    ///         queue.wait_for(|| num.load(Ordering::Relaxed) > 5).await;
    ///         println!("received wakeup!");
    ///     }
    /// });
    ///
    /// println!("poking task...");
    ///
    /// for i in 0..20 {
    ///     num.store(i, Ordering::Relaxed);
    ///     queue.wake();
    /// }
    ///
    /// waiter.await.unwrap();
    /// # }
    /// # test();
    /// ```
    pub async fn wait_for<F: FnMut() -> bool>(&self, mut f: F) -> Result<(), Closed> {
        loop {
            let wait = self.subscribe().await;
            if f() {
                return Ok(());
            }
            wait.await?;
        }
    }

    /// Asynchronously poll the given function `f` until a condition occurs,
    /// using the [`WaitCell`] to only re-poll when notified.
    ///
    /// This can be used to implement a "wait loop", turning a "try" function
    /// (e.g. "try_recv" or "try_send") into an asynchronous function (e.g.
    /// "recv" or "send").
    ///
    /// In particular, this function correctly *registers* interest in the [`WaitCell`]
    /// prior to polling the function, ensuring that there is not a chance of a race
    /// where the condition occurs AFTER checking but BEFORE registering interest
    /// in the [`WaitCell`], which could lead to deadlock.
    ///
    /// This is intended to have similar behavior to `Condvar` in the standard library,
    /// but asynchronous, and not requiring operating system intervention (or existence).
    ///
    /// In particular, this can be used in cases where interrupts or events are used
    /// to signify readiness or completion of some task, such as the completion of a
    /// DMA transfer, or reception of an ethernet frame. In cases like this, the interrupt
    /// can wake the cell, allowing the polling function to check status fields for
    /// partial progress or completion, and also return the status flags at the same time.
    ///
    /// Consider using [`Self::wait_for()`] if your function does not return a value.
    ///
    /// Consider using [`WaitQueue::wait_for_value()`](super::wait_queue::WaitQueue::wait_for_value) if you need multiple waiters.
    ///
    /// * [`Ok`]`(T)` if the closure returns [`Some`]`(T)`.
    /// * [`Err`]`(`[`Closed`]`)` if the [`WaitCell`] is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// use std::sync::Arc;
    /// use maitake_sync::WaitCell;
    /// use std::sync::atomic::{AtomicU8, Ordering};
    ///
    /// let queue = Arc::new(WaitCell::new());
    /// let num = Arc::new(AtomicU8::new(0));
    ///
    /// let waiter = task::spawn({
    ///     // clone items to move into the spawned task
    ///     let queue = queue.clone();
    ///     let num = num.clone();
    ///     async move {
    ///         let rxd = queue.wait_for_value(|| {
    ///             let val = num.load(Ordering::Relaxed);
    ///             if val > 5 {
    ///                 return Some(val);
    ///             }
    ///             None
    ///         }).await.unwrap();
    ///         assert!(rxd > 5);
    ///         println!("received wakeup with value: {rxd}");
    ///     }
    /// });
    ///
    /// println!("poking task...");
    ///
    /// for i in 0..20 {
    ///     num.store(i, Ordering::Relaxed);
    ///     queue.wake();
    /// }
    ///
    /// waiter.await.unwrap();
    /// # }
    /// # test();
    /// ```
    pub async fn wait_for_value<T, F: FnMut() -> Option<T>>(&self, mut f: F) -> Result<T, Closed> {
        loop {
            let wait = self.subscribe().await;
            if let Some(t) = f() {
                return Ok(t);
            }
            wait.await?;
        }
    }

    /// Returns `true` if this `WaitCell` is [closed](Self::close).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.current_state() == State::CLOSED
    }

    /// Takes this `WaitCell`'s waker.
    // TODO(eliza): could probably be made a public API...
    pub(crate) fn take_waker(&self, close: bool) -> Option<Waker> {
        trace!(wait_cell = ?fmt::ptr(self), ?close, "notifying");
        // Set the WAKING bit (to indicate that we're touching the waker) and
        // the WOKEN bit (to indicate that we intend to wake it up).
        let state = {
            let mut bits = State::WAKING | State::WOKEN;
            if close {
                bits.0 |= State::CLOSED.0;
            }
            test_dbg!(self.fetch_or(bits, AcqRel))
        };

        // Is anyone else touching the waker?
        if !test_dbg!(state.contains(State::WAKING | State::REGISTERING | State::CLOSED)) {
            // Ladies and gentlemen...we got him (the lock)!
            let waker = self.waker.with_mut(|thread| unsafe { (*thread).take() });

            // Release the lock.
            self.fetch_and(!State::WAKING, Release);

            if let Some(waker) = test_dbg!(waker) {
                trace!(wait_cell = ?fmt::ptr(self), ?close, ?waker, "notified");
                return Some(waker);
            }
        }

        None
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
        self.state
            .compare_exchange(curr, new, success, Acquire)
            .map(State)
            .map_err(State)
    }

    #[inline(always)]
    fn fetch_and(&self, State(state): State, order: Ordering) -> State {
        State(self.state.fetch_and(state, order))
    }

    #[inline(always)]
    fn fetch_or(&self, State(state): State, order: Ordering) -> State {
        State(self.state.fetch_or(state, order))
    }

    #[inline(always)]
    fn current_state(&self) -> State {
        State(self.state.load(Acquire))
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
    type Output = Result<(), Closed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        enter_test_debug_span!("Wait::poll");

        // Did a wakeup occur while we were pre-registering the future?
        if test_dbg!(self.presubscribe.is_ready()) {
            return self.presubscribe;
        }

        // Okay, actually poll the cell, then.
        match task::ready!(test_dbg!(self.cell.poll_wait(cx))) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(PollWaitError::Closed) => Poll::Ready(Err(Closed(()))),
            Err(PollWaitError::Busy) => {
                // If some other task was registering, yield and try to re-register
                // our waker when that task is done.
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

// === impl Subscribe ===

impl<'cell> Future for Subscribe<'cell> {
    type Output = Wait<'cell>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        enter_test_debug_span!("Subscribe::poll");

        // Pre-register the waker in the cell.
        let presubscribe = match test_dbg!(self.cell.poll_wait(cx)) {
            Poll::Ready(Err(PollWaitError::Busy)) => {
                // Someone else is in the process of registering. Yield now so we
                // can wait until that task is done, and then try again.
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(Err(PollWaitError::Closed)) => Poll::Ready(Err(Closed(()))),
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        };

        Poll::Ready(Wait {
            cell: self.cell,
            presubscribe,
        })
    }
}

// === impl State ===

impl State {
    /// /!\ EXTREMELY SERIOUS WARNING! /!\
    /// It is LOAD BEARING that the `WAITING` state is represented by zero!
    /// This is because we return to the waiting state by `fetch_and`ing out all
    /// other bits in a few places. If this state's bit representation is
    /// changed to anything other than zero, that code will break! Don't do
    /// that!
    ///
    /// YES, FUTURE ELIZA, THIS DOES APPLY TO YOU. YOU ALREADY BROKE IT ONCE.
    /// DON'T DO IT AGAIN.
    const WAITING: Self = Self(0b0000);
    const REGISTERING: Self = Self(0b0001);
    const WAKING: Self = Self(0b0010);
    const WOKEN: Self = Self(0b0100);
    const CLOSED: Self = Self(0b1000);

    fn contains(self, Self(state): Self) -> bool {
        self.0 & state > 0
    }
}

impl ops::BitOr for State {
    type Output = Self;

    fn bitor(self, Self(rhs): Self) -> Self::Output {
        Self(self.0 | rhs)
    }
}

impl ops::BitAnd for State {
    type Output = Self;

    fn bitand(self, Self(rhs): Self) -> Self::Output {
        Self(self.0 & rhs)
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

        fmt_bits!(self, f, has_states, REGISTERING, WAKING, CLOSED, WOKEN);

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
    use alloc::sync::Arc;

    use tokio_test::{assert_pending, assert_ready, assert_ready_ok, task};

    #[test]
    fn wait_smoke() {
        let _trace = crate::util::test::trace_init();

        let wait = Arc::new(WaitCell::new());

        let mut task = task::spawn({
            let wait = wait.clone();
            async move { wait.wait().await }
        });

        assert_pending!(task.poll());

        assert!(wait.wake());

        assert!(task.is_woken());
        assert_ready_ok!(task.poll());
    }

    /// Reproduces https://github.com/hawkw/mycelium/issues/449
    #[test]
    fn wait_spurious_poll() {
        let _trace = crate::util::test::trace_init();

        let cell = Arc::new(WaitCell::new());
        let mut task = task::spawn({
            let cell = cell.clone();
            async move { cell.wait().await }
        });

        assert_pending!(task.poll(), "first poll should be pending");
        assert_pending!(task.poll(), "second poll should be pending");

        cell.wake();

        assert_ready_ok!(task.poll(), "should have been woken");
    }

    #[test]
    fn subscribe() {
        let _trace = crate::util::test::trace_init();
        futures::executor::block_on(async {
            let cell = WaitCell::new();
            let wait = cell.subscribe().await;
            cell.wake();
            wait.await.unwrap();
        })
    }

    #[test]
    fn wake_before_subscribe() {
        let _trace = crate::util::test::trace_init();
        let cell = Arc::new(WaitCell::new());
        cell.wake();

        let mut task = task::spawn({
            let cell = cell.clone();
            async move {
                let wait = cell.subscribe().await;
                wait.await.unwrap();
            }
        });

        assert_ready!(task.poll(), "woken task should complete");

        let mut task = task::spawn({
            let cell = cell.clone();
            async move {
                let wait = cell.subscribe().await;
                wait.await.unwrap();
            }
        });

        assert_pending!(task.poll(), "wait cell hasn't been woken yet");
        cell.wake();
        assert!(task.is_woken());
        assert_ready!(task.poll());
    }

    #[test]
    fn wake_debounce() {
        let _trace = crate::util::test::trace_init();
        let cell = Arc::new(WaitCell::new());

        let mut task = task::spawn({
            let cell = cell.clone();
            async move {
                cell.wait().await.unwrap();
            }
        });

        assert_pending!(task.poll());
        cell.wake();
        cell.wake();
        assert!(task.is_woken());
        assert_ready!(task.poll());

        let mut task = task::spawn({
            let cell = cell.clone();
            async move {
                cell.wait().await.unwrap();
            }
        });

        assert_pending!(task.poll());
        assert!(!task.is_woken());

        cell.wake();
        assert!(task.is_woken());
        assert_ready!(task.poll());
    }

    #[test]
    fn subscribe_doesnt_self_wake() {
        let _trace = crate::util::test::trace_init();
        let cell = Arc::new(WaitCell::new());

        let mut task = task::spawn({
            let cell = cell.clone();
            async move {
                let wait = cell.subscribe().await;
                wait.await.unwrap();
                let wait = cell.subscribe().await;
                wait.await.unwrap();
            }
        });
        assert_pending!(task.poll());
        assert!(!task.is_woken());

        cell.wake();
        assert!(task.is_woken());
        assert_pending!(task.poll());

        assert!(!task.is_woken());
        assert_pending!(task.poll());

        cell.wake();
        assert!(task.is_woken());
        assert_ready!(task.poll());
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
                tracing::info!("waking");
                waker.wake();
                tracing::info!("woken");
            });
            thread::spawn(move || {
                tracing::info!("closing");
                closer.close();
                tracing::info!("closed");
            });

            tracing::info!("waiting");
            let _ = future::block_on(wait.wait());
            tracing::info!("wait'd");
        });
    }

    #[test]
    fn subscribe() {
        crate::loom::model(|| {
            future::block_on(async move {
                let cell = Arc::new(WaitCell::new());
                let wait = cell.subscribe().await;

                thread::spawn({
                    let waker = cell.clone();
                    move || {
                        tracing::info!("waking");
                        waker.wake();
                        tracing::info!("woken");
                    }
                });

                tracing::info!("waiting");
                wait.await.expect("wait should be woken, not closed");
                tracing::info!("wait'd");
            });
        });
    }
}
