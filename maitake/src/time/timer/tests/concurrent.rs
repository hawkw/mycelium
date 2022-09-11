use super::*;
use crate::loom::{sync::Arc, thread};
use core::{
    future::Future,
    task::{Context, Poll},
};
use futures_util::{future, pin_mut};

#[cfg(loom)]
use loom::future::block_on;
#[cfg(not(loom))]
use tokio_test::block_on;

#[cfg(loom)]
use loom::model;

#[cfg(not(loom))]
fn model(f: impl Fn() + Send + Sync + 'static) {
    crate::util::test::trace_init_with_default("info,maitake::time=trace");
    f();
}

#[test]
fn one_sleep() {
    model(|| {
        let timer = Arc::new(Timer::new(Duration::from_millis(1)));
        let thread = thread::spawn({
            let timer = timer.clone();
            move || {
                let sleep = timer.sleep(Duration::from_secs(1));
                block_on(sleep)
            }
        });

        for _ in 0..10 {
            timer.advance(Duration::from_secs(1));
            thread::yield_now();
        }

        thread.join().unwrap();
    })
}

#[test]
fn two_sleeps_parallel() {
    model(|| {
        let timer = Arc::new(Timer::new(Duration::from_millis(1)));
        let thread1 = thread::spawn({
            let timer = timer.clone();
            move || {
                let sleep = timer.sleep(Duration::from_secs(1));
                block_on(sleep)
            }
        });
        let thread2 = thread::spawn({
            let timer = timer.clone();
            move || {
                let sleep = timer.sleep(Duration::from_secs(1));
                block_on(sleep)
            }
        });

        for _ in 0..10 {
            timer.advance(Duration::from_secs(1));
            thread::yield_now();
        }

        thread1.join().unwrap();
        thread2.join().unwrap();
    })
}

#[test]
fn two_sleeps_sequential() {
    model(|| {
        let timer = Arc::new(Timer::new(Duration::from_millis(1)));
        let thread = thread::spawn({
            let timer = timer.clone();
            move || {
                block_on(async move {
                    timer.sleep(Duration::from_secs(1)).await;
                    timer.sleep(Duration::from_secs(1)).await;
                })
            }
        });

        for _ in 0..10 {
            timer.advance(Duration::from_secs(1));
            thread::yield_now();
        }

        thread.join().unwrap();
    })
}

#[test]
fn cancel_polled_sleeps() {
    fn poll_and_cancel(timer: Arc<Timer>) -> impl FnOnce() {
        move || {
            let timer = timer;
            block_on(async move {
                let sleep = timer.sleep_ticks(15);
                pin_mut!(sleep);
                future::poll_fn(move |cx| {
                    // poll once to register the sleep with the timer wheel, and
                    // then return `Ready` so it gets canceled.
                    let poll = sleep.as_mut().poll(cx);
                    tokio_test::assert_pending!(
                        poll,
                        "sleep should not complete, as its timer has not been advanced",
                    );
                    let poll = sleep.as_mut().poll(cx);
                    tokio_test::assert_pending!(
                        poll,
                        "sleep should not complete, as its timer has not been advanced",
                    );
                    Poll::Ready(())
                })
                .await
            });
        }
    }

    model(|| {
        let timer = Arc::new(Timer::new(Duration::from_secs(1)));
        let thread = thread::spawn(poll_and_cancel(timer.clone()));
        poll_and_cancel(timer)();
        thread.join().unwrap()
    })
}

#[test]
fn reregister_waker() {
    model(|| {
        let timer = Arc::new(Timer::new(Duration::from_millis(1)));
        let thread = thread::spawn({
            let timer = timer.clone();
            move || {
                let sleep = timer.sleep(Duration::from_secs(1));
                pin_mut!(sleep);
                // poll the sleep future initially with a no-op waker.
                let _ = sleep.as_mut().poll(&mut Context::from_waker(
                    futures_util::task::noop_waker_ref(),
                ));
                block_on(sleep)
            }
        });

        for _ in 0..10 {
            timer.advance(Duration::from_secs(1));
            thread::yield_now();
        }

        thread.join().unwrap();
    })
}
