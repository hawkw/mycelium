use super::*;
use crate::loom::{self, future::block_on, sync::Arc, thread};
use core::{task::Poll, future::Future};
use futures_util::{pin_mut, future};

#[test]
fn cancel_polled_sleeps() {
    fn poll_and_cancel(timer: Arc<Timer>) -> impl FnOnce() {
        move || {
            let timer = timer;
            block_on(async move {
                let sleep = timer.sleep(15);
                let mut polled = false;
                pin_mut!(sleep);
                future::poll_fn(move |cx| {
                    if polled {
                        return Poll::Ready(());
                    }

                    polled = true;
                    sleep.as_mut().poll(cx)
                }).await
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
