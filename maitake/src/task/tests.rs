use crate::{future, scheduler::Schedule, task::*};

#[derive(Copy, Clone, Debug)]
struct NopSchedule;

impl Schedule for NopSchedule {
    fn schedule(&self, task: TaskRef) {
        unimplemented!("nop scheduler tried to schedule task {:?}", task);
    }

    fn current_task(&self) -> Option<TaskRef> {
        unimplemented!("nop scheduler does not have a current task")
    }
}

#[cfg(loom)]
mod loom;

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc;

#[cfg(all(not(loom), feature = "alloc"))]
mod join_handle;
