use super::Task;
use mycelium_util::{intrusive::list, sync::spin};
pub(crate) struct TaskList<S> {
    inner: spin::Mutex<Inner<S>>,
}

struct Inner<S> {
    list: list::List<Task<S>>,
    closed: bool,
}
