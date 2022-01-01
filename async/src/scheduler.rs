use crate::{
    loom::sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Task, Waker},
};
use core::{any::type_name, ptr::NonNull};
use mycelium_util::{intrusive::list, sync::spin};

#[derive(Debug)]
pub struct StaticScheduler {
    inner: SchedulerInner<&'static Self>,
}

#[derive(Debug)]
struct SchedulerInner<S> {
    run_queue: spin::Mutex<list::List<Task<S>>>,
    woken: AtomicBool,
}

pub(crate) trait Schedule: Sized {
    fn schedule(&self, task: Pin<*const task::Header<Self>>);
    fn clone_ref(&self);
    fn drop_ref(&self);
}

impl Schedule for &'static StaticScheduler {
    fn schedule(&self, task: Pin<*const task::Header<Self>>) {
        self.inner.run_queue.lock().push_front(task);
        self.inner.woken.store(true, Ordering::Release);
    }

    fn clone_ref(&self) {
        // nop; the static scheduler needn't be ref counted.
    }

    fn drop_ref(&self) {
        // nop; the static scheduler needn't be ref counted.
    }
}
