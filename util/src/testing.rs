use core::fmt;

pub struct TestCase {
    pub name: &'static str,
    pub func: fn() -> Result<(), ()>,
}

pub trait TestResult {
    fn into_result(self) -> Result<(), ()>;
}

impl TestResult for () {
    fn into_result(self) -> Result<(), ()> {
        Ok(())
    }
}

impl<R: fmt::Debug> TestResult for Result<(), R> {
    fn into_result(self) -> Result<(), ()> {
        match self {
            Ok(_) => Ok(()),
            Err(err) => {
                tracing::error!(?err, "test failed");
                Err(())
            }
        }
    }
}
