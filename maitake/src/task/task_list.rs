use super::Task;
use maitake_sync::blocking;
use mycelium_util::intrusive::list;

pub(crate) struct TaskList {
    inner: blocking::Mutex<Inner>,
}

struct Inner {
    list: list::List<Task>,
    closed: bool,
}
