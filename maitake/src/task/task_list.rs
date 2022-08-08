use super::Task;
use mycelium_util::{intrusive::list, sync::spin};
pub(crate) struct TaskList {
    inner: spin::Mutex<Inner>,
}

struct Inner {
    list: list::List<Task>,
    closed: bool,
}
