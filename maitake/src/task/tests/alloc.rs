use super::*;
use crate::scheduler::Scheduler;
use ::alloc::boxed::Box;
use core::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

/// This test ensures that layout-dependent casts in the `Task` struct's
/// vtable methods are valid.
#[test]
fn task_is_valid_for_casts() {
    let task = Box::new(Task::<_, _, BoxStorage>::new(NopSchedule, async {
        unimplemented!("this task should never be polled")
    }));

    let task_ptr = Box::into_raw(task);

    // scope to ensure all task ptrs are dropped before we deallocate the
    // task allocation
    {
        let header_ptr = unsafe { ptr::addr_of!((*task_ptr).schedulable.header) };
        assert_eq!(
            task_ptr as *const (), header_ptr as *const (),
            "header pointer and task allocation pointer must have the same address!"
        );

        let sched_ptr = unsafe { ptr::addr_of!((*task_ptr).schedulable) };
        assert_eq!(
            task_ptr as *const (), sched_ptr as *const (),
            "schedulable pointer and task allocation pointer must have the same address!"
        );
    }

    // clean up after ourselves by ensuring the box is deallocated
    unsafe { drop(Box::from_raw(task_ptr)) }
}

/// This test just prints the size (in bytes) of an empty task struct.
#[test]
fn empty_task_size() {
    type Future = futures::future::Ready<()>;
    type EmptyTask = Task<NopSchedule, Future, BoxStorage>;
    println!(
        "{}: {}B",
        core::any::type_name::<EmptyTask>(),
        core::mem::size_of::<EmptyTask>(),
    );
    println!(
        "{}: {}B",
        core::any::type_name::<Future>(),
        core::mem::size_of::<Future>(),
    );
    println!(
        "task size: {}B",
        core::mem::size_of::<EmptyTask>() - core::mem::size_of::<Future>()
    );
}

#[test]
fn builder_works() {
    let scheduler = Scheduler::new();
    static IT_WORKED: AtomicBool = AtomicBool::new(false);
    scheduler.build_task().name("hello world").spawn(async {
        println!("hello world");
        IT_WORKED.store(true, Ordering::Relaxed);
    });

    scheduler.tick();
    assert!(IT_WORKED.load(Ordering::Acquire));
}
