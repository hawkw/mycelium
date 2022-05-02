use super::TaskRef;
use cordyceps::list;
use mycelium_util::sync::spin;
pub(crate) struct TaskList {
    inner: spin::Mutex<Inner>,
}

struct Inner {
    list: list::List<TaskRef>,
    closed: bool,
}
