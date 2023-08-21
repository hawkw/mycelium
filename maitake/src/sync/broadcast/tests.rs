use crate::sync::broadcast;
use crate::sync::broadcast::TryRecvError::{Closed, Empty, Lagged};

use crate::loom::{self, future::block_on, sync::Arc, thread};
// use tokio_test::{assert_err, assert_ok};

#[test]
fn broadcast_send() {
    loom::model(|| {
        let (tx1, mut rx) = broadcast::channel(2);
        let tx1 = Arc::new(tx1);
        let tx2 = tx1.clone();

        let th1 = thread::spawn(move || {
            block_on(async {
                test_dbg!(tx1.send("one").await).unwrap();
                test_dbg!(tx1.send("two").await).unwrap();
                test_dbg!(tx1.send("three").await).unwrap();
            });
        });

        let th2 = thread::spawn(move || {
            block_on(async {
                test_dbg!(tx2.send("eins").await).unwrap();
                test_dbg!(tx2.send("zwei").await).unwrap();
                test_dbg!(tx2.send("drei").await).unwrap();
            });
        });

        block_on(async {
            let mut num = 0;
            loop {
                match test_dbg!(rx.recv().await) {
                    Ok(_) => num += 1,
                    Err(Closed) => break,
                    Err(Lagged(n)) => num += n,
                    Err(Empty) => panic!("unexpected empty"),
                }
            }
            assert_eq!(num, 6);
        });

        th1.join().unwrap();
        th2.join().unwrap();
    });
}

// An `Arc` is used as the value in order to detect memory leaks.
#[test]
fn broadcast_two() {
    loom::model(|| {
        let (tx, mut rx1) = broadcast::channel::<Arc<&'static str>>(16);
        let mut rx2 = tx.subscribe();

        let th1 = thread::spawn(move || {
            block_on(async move {
                let v = test_dbg!(rx1.recv().await).unwrap();
                assert_eq!(*v, "hello");

                let v = test_dbg!(rx1.recv().await).unwrap();
                assert_eq!(*v, "world");

                match test_dbg!(rx1.recv().await).unwrap_err() {
                    Closed => {}
                    err => panic!("unexpected error: {err:?}"),
                }
            });
        });

        let th2 = thread::spawn(move || {
            block_on(async move {
                let v = test_dbg!(rx2.recv().await).unwrap();
                assert_eq!(*v, "hello");

                let v = test_dbg!(rx2.recv().await).unwrap();
                assert_eq!(*v, "world");

                match test_dbg!(rx2.recv().await).unwrap_err() {
                    Closed => {}
                    _ => panic!(),
                }
            });
        });
        block_on(async move {
            test_dbg!(tx.send(Arc::new("hello")).await).unwrap();
            test_dbg!(tx.send(Arc::new("world")).await).unwrap();
        });

        th1.join().unwrap();
        th2.join().unwrap();
    });
}

#[test]
fn broadcast_wrap() {
    loom::model(|| {
        let (tx, mut rx1) = broadcast::channel(2);
        let mut rx2 = tx.subscribe();

        let th1 = thread::spawn(move || {
            block_on(async {
                let mut num = 0;

                loop {
                    match test_dbg!(rx1.recv().await) {
                        Ok(_) => num += 1,
                        Err(Closed) => break,
                        Err(Lagged(n)) => num += n,
                        Err(Empty) => panic!("unexpected empty"),
                    }
                }

                assert_eq!(num, 3);
            });
        });

        let th2 = thread::spawn(move || {
            block_on(async {
                let mut num = 0;

                loop {
                    match test_dbg!(rx2.recv().await) {
                        Ok(_) => num += 1,
                        Err(Closed) => break,
                        Err(Lagged(n)) => num += n,
                        Err(Empty) => panic!("unexpected empty"),
                    }
                }

                assert_eq!(num, 3);
            });
        });

        block_on(async move {
            test_dbg!(tx.send("one").await).unwrap();
            test_dbg!(tx.send("two").await).unwrap();
            test_dbg!(tx.send("three").await).unwrap();
        });

        th1.join().unwrap();
        th2.join().unwrap();
    });
}

#[test]
fn drop_rx() {
    loom::model(|| {
        let (tx, mut rx1) = broadcast::channel(16);
        let rx2 = tx.subscribe();

        let th1 = thread::spawn(move || {
            block_on(async {
                let v = test_dbg!(rx1.recv().await).unwrap();
                assert_eq!(v, "one");

                let v = test_dbg!(rx1.recv().await).unwrap();
                assert_eq!(v, "two");

                let v = test_dbg!(rx1.recv().await).unwrap();
                assert_eq!(v, "three");

                match test_dbg!(rx1.recv().await).unwrap_err() {
                    Closed => {}
                    _ => panic!(),
                }
            });
        });

        let th2 = thread::spawn(move || {
            drop(rx2);
        });

        block_on(async move {
            test_dbg!(tx.send("one").await).unwrap();
            test_dbg!(tx.send("two").await).unwrap();
            test_dbg!(tx.send("three").await).unwrap();
        });

        th1.join().unwrap();
        th2.join().unwrap();
    });
}

#[test]
#[ignore]
fn drop_multiple_rx_with_overflow() {
    loom::model(move || {
        // It is essential to have multiple senders and receivers in this test case.
        let (tx, mut rx) = broadcast::channel(1);
        let _rx2 = tx.subscribe();

        let tx = block_on(async {
            let _ = test_dbg!(tx.send(()).await);
            tx
        });
        let tx2 = tx.clone();
        let th1 = thread::spawn(move || {
            block_on(async {
                for _ in 0..100 {
                    let _ = test_dbg!(tx2.send(()).await);
                }
            });
        });
        let tx = block_on(async {
            let _ = test_dbg!(tx.send(()).await);
            tx
        });
        let th2 = thread::spawn(move || {
            block_on(async { while let Ok(_) = test_dbg!(rx.recv().await) {} });
        });

        th1.join().unwrap();
        th2.join().unwrap();
    });
}
