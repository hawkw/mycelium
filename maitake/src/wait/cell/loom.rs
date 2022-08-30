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
