use super::*;

mycotest::decl_test! {
    fn wasm_hello_world() -> Result<(), wasmi::Error> {
        const HELLOWORLD_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/helloworld.wasm"));
        wasm::run_wasm(HELLOWORLD_WASM)
    }
}

mod alloc {
    mycotest::decl_test! {
        fn basic_alloc() -> mycotest::TestResult {
            // Let's allocate something, for funsies
            use alloc::vec::Vec;
            let mut v = Vec::new();
            tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
            v.push(5u64);
            tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
            v.push(10u64);
            tracing::info!(vec=?v, vec.addr=?v.as_ptr());
            mycotest::assert_eq!(v.pop(), Some(10));
            mycotest::assert_eq!(v.pop(), Some(5));

            Ok(())
        }
    }

    mycotest::decl_test! {
        fn alloc_big() {
            use alloc::vec::Vec;
            let mut v = Vec::new();

            for i in 0..2048 {
                v.push(i);
            }

            tracing::info!(vec = ?v);
        }
    }
}

mod myco_async {
    use core::{
        future::Future,
        pin::Pin,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        task::{Context, Poll},
    };
    use maitake::scheduler::StaticScheduler;
    use mycelium_util::sync::Lazy;

    struct Yield {
        yields: usize,
    }

    impl Future for Yield {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            let yields = &mut self.as_mut().yields;
            if *yields == 0 {
                return Poll::Ready(());
            }
            *yields -= 1;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    impl Yield {
        fn once() -> Self {
            Self::new(1)
        }

        fn new(yields: usize) -> Self {
            Self { yields }
        }
    }

    mycotest::decl_test! {
        fn basically_works() -> mycotest::TestResult {
            static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
            static IT_WORKED: AtomicBool = AtomicBool::new(false);

            SCHEDULER.spawn(async {
                Yield::once().await;
                IT_WORKED.store(true, Ordering::Release);
            });

            let tick = SCHEDULER.tick();

            mycotest::assert!(IT_WORKED.load(Ordering::Acquire));
            mycotest::assert_eq!(tick.completed, 1);
            mycotest::assert!(!tick.has_remaining);
            mycotest::assert_eq!(tick.polled, 2);

            Ok(())
        }
    }

    mycotest::decl_test! {
        fn schedule_many() -> mycotest::TestResult {
            static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
            static COMPLETED: AtomicUsize = AtomicUsize::new(0);

            const TASKS: usize = 10;

            for _ in 0..TASKS {
                SCHEDULER.spawn(async {
                    Yield::once().await;
                    COMPLETED.fetch_add(1, Ordering::SeqCst);
                })
            }

            let tick = SCHEDULER.tick();

            mycotest::assert_eq!(tick.completed, TASKS);
            mycotest::assert_eq!(tick.polled, TASKS * 2);
            mycotest::assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
            mycotest::assert!(!tick.has_remaining);

            Ok(())
        }
    }

    mycotest::decl_test! {
        fn many_yields() -> mycotest::TestResult {
            static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
            static COMPLETED: AtomicUsize = AtomicUsize::new(0);

            const TASKS: usize = 10;

            for i in 0..TASKS {
                SCHEDULER.spawn(async move {
                    Yield::new(i).await;
                    COMPLETED.fetch_add(1, Ordering::SeqCst);
                })
            }

            let tick = SCHEDULER.tick();

            mycotest::assert_eq!(tick.completed, TASKS);
            mycotest::assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
            mycotest::assert!(!tick.has_remaining);

            Ok(())
        }
    }
}
