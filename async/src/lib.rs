#![cfg_attr(not(test), no_std)]
extern crate alloc;

#[cfg(not(test))]
macro_rules! test_dbg {
    ($e:expr) => {
        $e
    };
}

#[cfg(all(test, not(loom)))]
macro_rules! test_dbg {
    ($e:expr) => {
        std::dbg!($e)
    };
}

#[cfg(all(loom, test))]
macro_rules! test_println {
    ($($arg:tt)*) => {
        crate::loom::traceln(format_args!(
            "[{:?} {:>30}:{:<3}] {}",
            crate::loom::thread::current().id(),
            file!(),
            line!(),
            format_args!($($arg)*),
        ))
    }
}

#[cfg(all(loom, test))]
macro_rules! test_dbg {
    ($e:expr) => {
        match $e {
            e => {
                test_println!("{} = {:?}", stringify!($e), &e);
                e
            }
        }
    };
}
pub(crate) mod loom;

pub mod scheduler;
pub mod task;
mod util;
