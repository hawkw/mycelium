#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
#![cfg_attr(target_os = "none", feature(asm))]
#![cfg_attr(target_os = "none", feature(panic_info_message, track_caller))]

pub mod arch;

#[cfg(target_os = "none")]
#[panic_handler]
#[cold]
fn panic(panic: &core::panic::PanicInfo) -> ! {
    use core::fmt;

    struct PrettyPanic<'a>(&'a core::panic::PanicInfo<'a>);
    impl<'a> fmt::Display for PrettyPanic<'a> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let message = self.0.message();
            let location = self.0.location();
            let caller = core::panic::Location::caller();
            if let Some(message) = message {
                writeln!(f, "  mycelium panicked: {}", message)?;
                if let Some(loc) = location {
                    writeln!(f, "  at: {}:{}:{}", loc.file(), loc.line(), loc.column(),)?;
                }
            } else {
                writeln!(f, "  mycelium panicked: {}", self.0)?;
            }
            writeln!(
                f,
                "  in: {}:{}:{}",
                caller.file(),
                caller.line(),
                caller.column()
            )?;
            Ok(())
        }
    }

    let caller = core::panic::Location::caller();
    tracing::error!(%panic, ?caller);
    let pp = PrettyPanic(panic);
    arch::oops(&pp)
}

fn main() {
    unsafe {
        core::hint::unreachable_unchecked();
    }
}
