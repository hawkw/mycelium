#![cfg_attr(not(any(loom, feature = "alloc")), allow(dead_code))]
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

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests;
#[cfg(any(loom, feature = "alloc"))]
mod loom;
