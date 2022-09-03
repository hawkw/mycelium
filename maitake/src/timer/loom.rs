use super::*;
use crate::loom::{self, future::block_on, sync::Arc, thread};
use core::{future::Future, task::Poll};
use futures_util::{future, pin_mut};

#[test]
fn cancel_polled_sleeps() {
    fn poll_and_cancel(timer: Arc<Timer>) -> impl FnOnce() {
        move || {
            let timer = timer;
            block_on(async move {
                let sleep = timer.sleep(15);
                pin_mut!(sleep);
                future::poll_fn(move |cx| {
                    // poll once to register the sleep with the timer wheel, and
                    // then return `Ready` so it gets canceled.
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

    loom::model(|| {
        let timer = Arc::new(Timer::new());
        let thread = thread::spawn(poll_and_cancel(timer.clone()));
        poll_and_cancel(timer)();
        thread.join().unwrap()
    })
}
