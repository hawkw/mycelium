use super::TaskRef;
use core::marker::PhantomData;

pub struct JoinHandle<T> {
    task: Option<TaskRef>,
    _t: PhantomData<fn(T)>,
}

#[non_exhaustive]
pub enum JoinError {
    Canceled,
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        // if the JoinHandle has not already been consumed, clear the join
        // handle flag on the task.
        if let Some(ref task) = self.task {
            task.state().drop_join_handle();
        }
    }
}
