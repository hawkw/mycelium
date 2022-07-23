use super::*;
use core::{future::Future, ptr::NonNull};
use maitake::task::{self, Task};

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

impl<F: Future + 'static> MyBoxTask<&'static StaticScheduler, F> {
    fn spawn(scheduler: &'static StaticScheduler, future: F) {
        let task = MyBoxTask(Box::new(Task::new(scheduler, future)));
        scheduler.spawn_allocated::<F, MyBoxStorage>(task)
    }
}

#[test]
fn basically_works() {
    static STUB: TaskStub = TaskStub::new();
    static SCHEDULER: StaticScheduler = unsafe { StaticScheduler::new_with_static_stub(&STUB) };
    static IT_WORKED: AtomicBool = AtomicBool::new(false);

    util::trace_init();

    MyBoxTask::spawn(&SCHEDULER, async {
        future::yield_now().await;
        IT_WORKED.store(true, Ordering::Release);
    });

    let tick = SCHEDULER.tick();

    assert!(IT_WORKED.load(Ordering::Acquire));
    assert_eq!(tick.completed, 1);
    assert!(!tick.has_remaining);
    assert_eq!(tick.polled, 2)
}

#[test]
fn schedule_many() {
    static STUB: TaskStub = TaskStub::new();
    static SCHEDULER: StaticScheduler = unsafe { StaticScheduler::new_with_static_stub(&STUB) };
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    const TASKS: usize = 10;

    util::trace_init();

    for _ in 0..TASKS {
        MyBoxTask::spawn(&SCHEDULER, async {
            future::yield_now().await;
            COMPLETED.fetch_add(1, Ordering::SeqCst);
        })
    }

    let tick = SCHEDULER.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(tick.polled, TASKS * 2);
    assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
    assert!(!tick.has_remaining);
}

#[test]
fn many_yields() {
    static STUB: TaskStub = TaskStub::new();
    static SCHEDULER: StaticScheduler = unsafe { StaticScheduler::new_with_static_stub(&STUB) };
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    util::trace_init();
    const TASKS: usize = 10;

    for i in 0..TASKS {
        MyBoxTask::spawn(&SCHEDULER, async move {
            future::Yield::new(i).await;
            COMPLETED.fetch_add(1, Ordering::SeqCst);
        })
    }

    let tick = SCHEDULER.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
    assert!(!tick.has_remaining);
}
