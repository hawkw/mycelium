use super::{Context, JoinError, Poll, TaskRef};
use core::{future::Future, marker::PhantomData, pin::Pin};

#[derive(Debug)]
pub struct JoinHandle<T> {
    task: Option<TaskRef>,
    _t: PhantomData<fn(T)>,
}

impl<T> JoinHandle<T> {
    /// # Safety
    ///
    /// The pointed type must actually output a `T`-typed value.
    pub(super) unsafe fn from_task_ref(task: TaskRef) -> Self {
        task.state().create_join_handle();
        Self {
            task: Some(task),
            _t: PhantomData,
        }
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = Result<T, JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();
        let task = this.task.take().unwrap();
        let poll = unsafe {
            // Safety: the `JoinHandle` must have been constructed with the
            // task's actual output type!
            task.poll_join::<T>(cx)
        };
        if poll.is_pending() {
            this.task = Some(task);
        } else {
            // clear join interest
            task.state().drop_join_handle();
        }
        poll
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        test_trace!("drop JoinHandle");
        // if the JoinHandle has not already been consumed, clear the join
        // handle flag on the task.
        if let Some(ref task) = self.task {
            task.state().drop_join_handle();
        }
    }
}
