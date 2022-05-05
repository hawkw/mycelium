#![cfg_attr(not(test), no_std)]
extern crate alloc;

macro_rules! test_println {
    ($($arg:tt)*) => {
        #[cfg(test)]
        crate::loom::traceln(format_args!(
            "[{:?} {:>30}:{:<3}] {}",
            crate::loom::thread::current().id(),
            file!(),
            line!(),
            format_args!($($arg)*),
        ))
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
