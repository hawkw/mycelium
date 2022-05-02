use super::Task;
use cordyceps::list;
use mycelium_util::sync::spin;
pub(crate) struct TaskList<S> {
    inner: spin::Mutex<Inner<S>>,
}

struct Inner<S> {
    list: list::List<Task<S>>,
    closed: bool,
}
