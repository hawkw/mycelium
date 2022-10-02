use super::*;
use crate::loom::sync::atomic::{AtomicUsize, Ordering};
use crate::task::{self, Task};
use core::ptr::NonNull;
/// A fake custom task allocation.
///
/// This is actually just backed by `Box`, because we depend on `std` for
/// tests, but it could be implemented with a custom allocator type.
#[repr(transparent)]
struct MyBoxTask<S, F: Future>(Box<Task<S, F, MyBoxStorage>>);

struct MyBoxStorage;

impl<S, F: Future> task::Storage<S, F> for MyBoxStorage {
    type StoredTask = MyBoxTask<S, F>;
    fn into_raw(MyBoxTask(task): Self::StoredTask) -> NonNull<Task<S, F, Self>> {
        NonNull::new(Box::into_raw(task)).expect("box must never be null!")
    }

    fn from_raw(ptr: NonNull<Task<S, F, Self>>) -> Self::StoredTask {
        unsafe { MyBoxTask(Box::from_raw(ptr.as_ptr())) }
    }
}

impl<F> MyBoxTask<&'static StaticScheduler, F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn spawn(scheduler: &'static StaticScheduler, future: F) -> task::JoinHandle<F::Output> {
        let task = MyBoxTask(Box::new(Task::new(future)));
        scheduler.spawn_allocated::<F, MyBoxStorage>(task)
    }
}

#[test]
fn notify_future() {
    static STUB: TaskStub = TaskStub::new();
    static SCHEDULER: StaticScheduler = unsafe { StaticScheduler::new_with_static_stub(&STUB) };
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let _trace = crate::util::trace_init();
    let chan = Chan::new(1);

    MyBoxTask::spawn(&SCHEDULER, {
        let chan = chan.clone();
        async move {
            chan.wait().await;
            COMPLETED.fetch_add(1, Ordering::SeqCst);
        }
    });

    MyBoxTask::spawn(&SCHEDULER, async move {
        future::yield_now().await;
        chan.wake();
    });

    dbg!(SCHEDULER.tick());

    assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
}

#[test]
fn notify_external() {
    static STUB: TaskStub = TaskStub::new();
    static SCHEDULER: StaticScheduler = unsafe { StaticScheduler::new_with_static_stub(&STUB) };
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let _trace = crate::util::trace_init();
    let chan = Chan::new(1);

    MyBoxTask::spawn(&SCHEDULER, {
        let chan = chan.clone();
        async move {
            chan.wait().await;
            COMPLETED.fetch_add(1, Ordering::SeqCst);
        }
    });

    std::thread::spawn(move || {
        chan.wake();
    });

    while dbg!(SCHEDULER.tick().completed) < 1 {
        std::thread::yield_now();
    }

    assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
}
