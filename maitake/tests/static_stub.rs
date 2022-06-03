use maitake::scheduler::TaskStub;

/// This test reproduces an internal compiler error:
/// https://github.com/rust-lang/rust/issues/97708
#[test]
fn compiles() {
    #[allow(dead_code)]
    static TASK_STUB: TaskStub = TaskStub::new();
}
