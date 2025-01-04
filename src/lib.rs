#![cfg_attr(all(target_os = "none", test), no_main)]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
#![allow(unused_unsafe)]
#![doc = include_str!("../README.md")]

extern crate alloc;
extern crate rlibc;

pub mod allocator;
pub mod arch;
pub mod drivers;
pub mod rt;
pub mod shell;
pub mod wasm;

#[cfg(test)]
mod tests;

use core::fmt::Write;
use hal_core::{boot::BootInfo, mem};

pub const MYCELIUM_VERSION: &str = concat!(
    env!("VERGEN_BUILD_SEMVER"),
    "-",
    env!("VERGEN_GIT_BRANCH"),
    ".",
    env!("VERGEN_GIT_SHA_SHORT")
);

#[cfg_attr(target_os = "none", global_allocator)]
static ALLOC: allocator::Allocator = allocator::Allocator::new();

pub fn kernel_start(bootinfo: impl BootInfo, archinfo: crate::arch::ArchInfo) -> ! {
    let mut writer = bootinfo.writer();
    writeln!(
        writer,
        "hello from mycelium {} (on {})",
        MYCELIUM_VERSION,
        arch::NAME
    )
    .unwrap();
    writeln!(writer, "booting via {}", bootinfo.bootloader_name()).unwrap();

    if let Some(subscriber) = bootinfo.subscriber() {
        tracing::dispatch::set_global_default(subscriber).unwrap();
    }

    #[cfg(not(test))]
    if let Some(mut framebuf) = bootinfo.framebuffer() {
        use hal_core::framebuffer::{Draw, RgbColor};
        tracing::trace!("framebuffer exists!");
        framebuf.clear();
        let mut color = 0;
        for row in 0..framebuf.height() {
            color += 1;
            match color {
                1 => {
                    framebuf.fill_row(row, RgbColor::RED);
                }
                2 => {
                    framebuf.fill_row(row, RgbColor::GREEN);
                }
                3 => {
                    framebuf.fill_row(row, RgbColor::BLUE);
                    color = 0;
                }
                _ => {}
            }
        }
        tracing::trace!("made it grey!");

        use embedded_graphics::{
            mono_font::{ascii, MonoTextStyle},
            pixelcolor::{Rgb888, RgbColor as _},
            prelude::*,
            text::{Alignment, Text},
        };
        let center = Point::new(
            (framebuf.width() / 2) as i32,
            (framebuf.height() / 2) as i32,
        );
        let mut target = framebuf.clear().as_draw_target();
        let small_style = MonoTextStyle::new(&ascii::FONT_6X10, Rgb888::WHITE);
        Text::with_alignment(
            "the glorious\n",
            Point::new(center.x, center.y - 20),
            small_style,
            Alignment::Center,
        )
        .draw(&mut target)
        .expect("never panics");
        Text::with_alignment(
            "MYCELIUM\n",
            center,
            MonoTextStyle::new(&ascii::FONT_10X20, Rgb888::WHITE),
            Alignment::Center,
        )
        .draw(&mut target)
        .expect("never panics");
        Text::with_alignment(
            concat!("operating system\n\n v", env!("CARGO_PKG_VERSION"), "\n"),
            Point::new(center.x, center.y + 10),
            small_style,
            Alignment::Center,
        )
        .draw(&mut target)
        .expect("never panics");

        tracing::trace!("hahahaha yayyyy we drew a screen!");
    }

    arch::interrupt::enable_exceptions();
    bootinfo.init_paging();
    ALLOC.init(&bootinfo);

    let mut regions = 0;
    let mut free_regions = 0;
    let mut free_bytes = 0;

    {
        let span = tracing::info_span!("memory map");
        let _enter = span.enter();
        for region in bootinfo.memory_map() {
            let kind = region.kind();
            let size = region.size();
            tracing::info!(
                "  {:>10?} {:>15?} {:>15?} B",
                region.base_addr(),
                kind,
                size,
            );
            regions += 1;
            if region.kind() == mem::RegionKind::FREE {
                free_regions += 1;
                free_bytes += size;
                unsafe {
                    ALLOC.add_region(region);
                }
            }
        }

        tracing::info!(
            "found {} memory regions, {} free regions ({} bytes)",
            regions,
            free_regions,
            free_bytes,
        );
    }

    // perform arch-specific initialization once we have an allocator and
    // tracing.
    let clock = arch::init(&bootinfo, &archinfo);

    // initialize the kernel runtime.
    rt::init(clock);

    #[cfg(test)]
    arch::run_tests();

    kernel_main(bootinfo);
}

fn kernel_main(bootinfo: impl BootInfo) -> ! {
    rt::spawn(keyboard_demo());

    let mut core = rt::Core::new();
    tracing::info!(
        version = %MYCELIUM_VERSION,
        arch = %arch::NAME,
        bootloader = %bootinfo.bootloader_name(),
        "welcome to the Glorious Mycelium Operating System",
    );

    loop {
        core.run();
        tracing::warn!("someone stopped CPU 0's core! restarting it...");
    }
}

#[cfg_attr(target_os = "none", alloc_error_handler)]
pub fn alloc_error(layout: core::alloc::Layout) -> ! {
    arch::oops(arch::Oops::alloc_error(layout))
}

#[cfg_attr(target_os = "none", panic_handler)]
#[cold]
pub fn panic(panic: &core::panic::PanicInfo<'_>) -> ! {
    arch::oops(arch::Oops::from(panic))
}

#[cfg(all(test, not(target_os = "none")))]
pub fn main() {
    /* no host-platform tests in this crate */
}

/// Keyboard handler demo task: logs each line typed by the user.
// TODO(eliza): let's do something Actually Useful with keyboard input...
async fn keyboard_demo() {
    #[cfg(target_os = "none")]
    use alloc::string::String;
    use drivers::ps2_keyboard::{self, DecodedKey, KeyCode};

    let mut line = String::new();
    tracing::info!("type `help` to list available commands");
    loop {
        let key = ps2_keyboard::next_key().await;
        let mut newline = false;
        match key {
            DecodedKey::Unicode('\n') | DecodedKey::Unicode('\r') => {
                newline = true;
            }
            // backspace
            DecodedKey::RawKey(KeyCode::Backspace)
            | DecodedKey::RawKey(KeyCode::Delete)
            | DecodedKey::Unicode('\u{0008}') => {
                line.pop();
            }
            DecodedKey::Unicode(c) => line.push(c),
            DecodedKey::RawKey(key) => tracing::warn!(?key, "you typed something weird"),
        }
        if newline {
            shell::eval(&line);
            line.clear();
        }
    }
}
