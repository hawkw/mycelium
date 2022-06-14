pub mod mutex;
#[doc(inline)]
pub use self::mutex::{Mutex, MutexGuard, OwnedMutexGuard};

#[cfg(test)]
mod tests;
