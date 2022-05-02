use crate::task::{Context, Header, Poll, Task, Waker};
use mycelium_util::{intrusive::list, sync::spin};

pub struct Scheduler {
    tasks: spin::Mutex<list::List<Task>>,
}
