use crate::{
    loom::sync::atomic::{AtomicBool, Ordering},
    task::{self, Context, Poll, TaskRef, Waker},
};
use cordyceps::mpsc_queue::MpscQueue;
use core::{any::type_name, pin::Pin, ptr::NonNull};

#[derive(Debug)]
pub struct StaticScheduler {
    inner: SchedulerInner,
}

#[derive(Debug)]
struct SchedulerInner {
    run_queue: MpscQueue<TaskRef>,
    woken: AtomicBool,
}

pub(crate) trait Schedule: Sized {
    fn schedule(&self, task: NonNull<TaskRef>);
    fn clone_ref(&self);
    fn drop_ref(&self);
}

impl Schedule for &'static StaticScheduler {
    fn schedule(&self, task: NonNull<TaskRef>) {
        // self.inner.run_queue.lock().push_front(task);
        self.inner.run_queue.enqueue(task);
        self.inner.woken.store(true, Ordering::Release);
    }

    fn clone_ref(&self) {
        // nop; the static scheduler needn't be ref counted.
    }

    fn drop_ref(&self) {
        // nop; the static scheduler needn't be ref counted.
    }
}
