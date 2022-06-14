mod mutex;
pub use self::mutex::{Mutex, MutexGuard};

#[cfg(test)]
mod tests;
