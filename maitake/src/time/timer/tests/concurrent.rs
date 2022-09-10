use super::*;

use crate::loom::{sync::Arc, thread};
#[cfg(loom)]
use loom::future::block_on;
#[cfg(not(loom))]
use tokio_test::block_on;

#[cfg(loom)]
use loom::model;

#[cfg(not(loom))]
fn model(f: impl Fn() + Send + Sync + 'static) {
    f();
}

#[test]
fn one_sleep() {
    model(|| {
        let timer = Arc::new(Timer::new(Duration::from_millis(1)));
        let jh = thread::spawn(move || {
            let sleep = timer.sleep(Duration::from_secs(1));
            block_on(sleep)
        });

        thread::yield_now();

        timer.advance(Duration::from_secs(10));

        jh.join().unwrap();
    })
}
