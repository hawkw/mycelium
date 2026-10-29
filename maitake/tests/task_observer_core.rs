#![cfg(not(loom))]
use maitake::{
    scheduler::{StaticScheduler, TaskStub},
    task::{Storage, Task, TaskPhase},
};
use std::{
    future::Future,
    ptr::NonNull,
    task::{Context, Poll},
};

// A consumer-provided storage implementation lets this test use maitake with
// its alloc and std features disabled.
struct ConsumerStorage;
impl<S, F: Future> Storage<S, F> for ConsumerStorage {
    type StoredTask = Box<Task<S, F, Self>>;
    fn into_raw(task: Self::StoredTask) -> NonNull<Task<S, F, Self>> {
        NonNull::new(Box::into_raw(task)).unwrap()
    }
    fn from_raw(ptr: NonNull<Task<S, F, Self>>) -> Self::StoredTask {
        // Storage owns the allocation until the final task reference is gone.
        unsafe { Box::from_raw(ptr.as_ptr()) }
    }
}
#[test]
fn observers_work_with_consumer_storage_without_library_alloc() {
    static SCHEDULER: StaticScheduler = {
        static STUB: TaskStub = TaskStub::new();
        // This stub is used by exactly this scheduler.
        unsafe { StaticScheduler::new_with_static_stub(&STUB) }
    };
    let raw = Box::new(Task::<&StaticScheduler, _, ConsumerStorage>::new(async {
        6
    }));
    assert!(!raw.status().has_join_handle());
    let j = SCHEDULER.spawn_allocated::<_, ConsumerStorage>(raw);
    let t = j.task_ref();
    let mut o = t.observe();
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(
        matches!(Box::pin(o.changed()).as_mut().poll(&mut cx), Poll::Ready(Some(v)) if v.phase() == TaskPhase::Scheduled)
    );
    SCHEDULER.tick();
    assert!(
        matches!(Box::pin(o.changed()).as_mut().poll(&mut cx), Poll::Ready(Some(v)) if v.is_completed())
    );
    assert!(matches!(
        Box::pin(j).as_mut().poll(&mut cx),
        Poll::Ready(Ok(6))
    ));
    assert!(!o.status().has_output());
}
