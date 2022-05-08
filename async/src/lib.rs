#![cfg_attr(not(test), no_std)]
extern crate alloc;

macro_rules! event {
    ($($arg:tt)*) => {
        #[cfg(all(test, loom))]{
            use tracing::Level;
            tracing::event!($($arg)*);
        }
        #[cfg(all(test, loom))]
        {
            use tracing_01::Level;
            tracing_01::event!($($arg)*);
        }
    }
}

#[cfg(not(test))]
macro_rules! test_dbg {
    ($e:expr) => {
        $e
    };
}

#[cfg(test)]
macro_rules! test_dbg {
    ($e:expr) => {
        match $e {
            e => {
                event!(Level::DEBUG, "{} = {:?}", stringify!($e), &e);
                e
            }
        }
    };
}

pub(crate) mod loom;

pub mod scheduler;
pub mod task;
mod util;
