use crate::{serial, vga};
use log::{Level, LevelFilter, Log, Metadata, Record};

// #[derive(Debug)]
pub struct Logger {
    vga_max_level: LevelFilter,
    serial: Option<Serial>,
}

struct Serial {
    port: &'static serial::Port,
    max_level: LevelFilter,
}

impl Default for Logger {
    fn default() -> Self {
        Self {
            vga_max_level: LevelFilter::Info,
            serial: serial::com1().map(|port| Serial {
                port,
                max_level: LevelFilter::Trace,
            }),
        }
    }
}

impl Logger {
    pub const fn vga_only(vga_max_level: LevelFilter) -> Self {
        Self {
            vga_max_level,
            serial: None,
        }
    }

    #[inline]
    fn vga_enabled(&self, level: Level) -> bool {
        level <= self.vga_max_level
    }

    #[inline]
    fn serial_enabled(&self, level: Level) -> bool {
        self.serial
            .as_ref()
            .map(|serial| level <= serial.max_level)
            .unwrap_or(false)
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let level = metadata.level();
        self.vga_enabled(level) || self.serial_enabled(level)
    }

    fn log(&self, record: &Record) {
        let level = record.level();
        if self.vga_enabled(level) {
            use core::fmt::Write;
            let mut vga = vga::writer();

            print_level_vga(&level, &mut vga);
            vga.set_color(DEFAULT_COLOR);
            writeln!(vga, "] {:?}", record.args()).unwrap();
        }

        if let Some(ref serial) = self.serial {
            use mycelium_util::io::Write;
            if level <= serial.max_level {
                writeln!(
                    &mut serial.port.lock(),
                    "[{}] {}: {:?}",
                    level_to_str(&level),
                    record.target(),
                    record.args()
                )
                .unwrap();
            }
        }
    }

    fn flush(&self) {
        if let Some(ref serial) = self.serial {
            use mycelium_util::io::Write;
            serial.port.lock().flush().unwrap();
        }
    }
}

const VGA_BG: vga::Color = vga::Color::Black;
const DEFAULT_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::Green, VGA_BG);

fn level_to_str(level: &Level) -> &'static str {
    match *level {
        Level::Trace => "TRACE",
        Level::Debug => "DEBUG",
        Level::Info => " INFO",
        Level::Warn => " WARN",
        Level::Error => "ERROR",
    }
}

fn print_level_vga(level: &Level, vga: &mut vga::Writer) {
    use core::fmt::Write;
    const TRACE_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::Magenta, VGA_BG);
    const DEBUG_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::LightBlue, VGA_BG);
    const INFO_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::White, VGA_BG);
    const WARN_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::Yellow, VGA_BG);
    const ERR_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::Red, VGA_BG);

    vga.set_color(DEFAULT_COLOR);
    vga.write_char('[').unwrap();
    match *level {
        Level::Trace => {
            vga.set_color(TRACE_COLOR);
            vga.write_str("TRACE").unwrap();
            vga.set_color(DEFAULT_COLOR);
            vga.write_char(']').unwrap();
        }
        Level::Debug => {
            vga.set_color(DEBUG_COLOR);
            vga.write_str("DEBUG").unwrap();
            vga.set_color(DEFAULT_COLOR);
            vga.write_char(']').unwrap();
        }
        Level::Info => {
            vga.set_color(INFO_COLOR);
            vga.write_str("INFO").unwrap();
            vga.set_color(DEFAULT_COLOR);
            vga.write_str("] ").unwrap();
        }
        Level::Warn => {
            vga.set_color(WARN_COLOR);
            vga.write_str("WARN").unwrap();
            vga.set_color(DEFAULT_COLOR);
            vga.write_str("] ").unwrap();
        }
        Level::Error => {
            vga.set_color(ERR_COLOR);
            vga.write_str("ERROR").unwrap();
            vga.set_color(DEFAULT_COLOR);
            vga.write_str("]").unwrap();
        }
    };
}
