#![cfg_attr(all(target_os = "none", test), no_main)]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
#![feature(panic_info_message)]
#![allow(unused_unsafe)]
// we need the SPICY version of const-eval apparently
#![feature(const_mut_refs)]
#![doc = include_str!("../README.md")]

extern crate alloc;
extern crate rlibc;

pub mod arch;
pub mod wasm;

use core::fmt::Write;
use hal_core::{boot::BootInfo, mem};
use maitake::scheduler::{self, StaticScheduler};
use mycelium_alloc::buddy;

#[cfg(test)]
mod tests;

#[cfg_attr(target_os = "none", global_allocator)]
static ALLOC: buddy::Alloc<32> = buddy::Alloc::new(32);
static SCHEDULER: StaticScheduler = scheduler::new_static!();

pub fn kernel_start(bootinfo: &impl BootInfo) -> ! {
    let mut writer = bootinfo.writer();
    writeln!(
        writer,
        "hello from mycelium {} (on {})",
        env!("CARGO_PKG_VERSION"),
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

    arch::init_interrupts();
    bootinfo.init_paging();

    // XXX(eliza): this sucks
    ALLOC.set_vm_offset(arch::mm::vm_offset());

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
                    tracing::trace!(?region, "adding to page allocator");
                    let e = ALLOC.add_region(region);
                    tracing::trace!(added = e.is_ok());
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

    #[cfg(test)]
    arch::run_tests();

    kernel_main();
}

fn kernel_main() -> ! {
    tracing::info!("started kernel main loop");
    SCHEDULER.spawn(async move {
        use maitake::time;
        let duration = time::Duration::from_secs(5);
        loop {
            time::sleep(duration).await;
            tracing::info!(?duration, "slept");
        }
    });

    tracing::info!("spawned sleep task");
    loop {
        // drive the task scheduler
        let tick = SCHEDULER.tick();
        if tick.polled > 0 {
            tracing::trace!(
                tick.polled,
                tick.completed,
                tick.spawned,
                tick.woken_external,
                tick.woken_internal,
                tick.has_remaining,
            );
        }

        // turn the timer wheel if it wasn't turned recently, to ensure any
        // pending ticks are consumed.
        arch::tick_timer();
    }
}

#[cfg_attr(target_os = "none", alloc_error_handler)]
pub fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("alloc error: {:?}", layout);
}

#[cfg_attr(target_os = "none", panic_handler)]
#[cold]
pub fn panic(panic: &core::panic::PanicInfo<'_>) -> ! {
    // use core::fmt;
    // struct PrettyPanic<'a>(&'a core::panic::PanicInfo<'a>);
    // impl<'a> fmt::Display for PrettyPanic<'a> {
    //     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    //         let message = self.0.message();
    //         let location = self.0.location();
    //         if let Some(message) = message {
    //             writeln!(f, "  mycelium panicked: {}", message)?;
    //             if let Some(loc) = location {
    //                 writeln!(f, "  at: {}:{}:{}", loc.file(), loc.line(), loc.column(),)?;
    //             } else {
    //                 writeln!(f, "  at: ???")?;
    //             }
    //         } else {
    //             writeln!(f, "  mycelium panicked: {}", self.0)?;
    //         }
    //         Ok(())
    //     }
    // }

    // let pp = PrettyPanic(panic);
    arch::oops(arch::Oops::from(panic))
}

#[cfg(all(test, not(target_os = "none")))]
pub fn main() {
    /* no host-platform tests in this crate */
}
