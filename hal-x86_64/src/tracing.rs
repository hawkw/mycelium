use crate::{serial, vga};
use core::{
    fmt::{self, Write},
    sync::atomic::{AtomicU64, Ordering},
};
use tracing::{field, level_filters::LevelFilter, span, Event, Level, Metadata};

pub struct Subscriber {
    vga_max_level: LevelFilter,
    vga_indent: AtomicU64,
    serial: Option<Serial>,
    next_id: AtomicU64,
}

struct Serial {
    port: &'static serial::Port,
    max_level: LevelFilter,
    indent: AtomicU64,
}

impl Default for Subscriber {
    fn default() -> Self {
        Self {
            vga_max_level: LevelFilter::INFO,
            vga_indent: AtomicU64::new(0),
            serial: serial::com1().map(|port| Serial {
                port,
                max_level: LevelFilter::TRACE,
                indent: AtomicU64::new(0),
            }),
            next_id: AtomicU64::new(0),
        }
    }
}

const SERIAL_BIT: u64 = 1 << 62;
const VGA_BIT: u64 = 1 << 63;
const _ACTUAL_ID_BITS: u64 = !(SERIAL_BIT | VGA_BIT);

impl Subscriber {
    pub const fn vga_only(vga_max_level: LevelFilter) -> Self {
        Self {
            vga_max_level,
            vga_indent: AtomicU64::new(0),
            serial: None,
            next_id: AtomicU64::new(0),
        }
    }

    #[inline]
    fn vga_enabled(&self, level: &Level) -> bool {
        level <= &self.vga_max_level
    }

    #[inline]
    fn serial_enabled(&self, level: &Level) -> bool {
        self.serial
            .as_ref()
            .map(|serial| level <= &serial.max_level)
            .unwrap_or(false)
    }

    fn writer(&self, level: &Level) -> Writer<'_> {
        let vga = if self.vga_enabled(level) {
            Some((vga::writer(), &self.vga_indent))
        } else {
            None
        };
        let serial = if self.serial_enabled(level) {
            self.serial.as_ref().map(|s| (s.port.lock(), &s.indent))
        } else {
            None
        };
        Writer { vga, serial }
    }
}

struct Writer<'a> {
    vga: Option<(vga::Writer, &'a AtomicU64)>,
    serial: Option<(serial::Lock<'a>, &'a AtomicU64)>,
}

struct Visitor<'a, W> {
    writer: &'a mut W,
    seen: bool,
}

impl<'a> Write for Writer<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some((ref mut vga, _)) = self.vga {
            vga.write_str(s)?;
        }
        if let Some((ref mut serial, _)) = self.serial {
            serial.write_str(s)?;
        }
        Ok(())
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        if let Some((ref mut vga, _)) = self.vga {
            write!(vga, "{:?}", args)?;
        }
        if let Some((ref mut serial, _)) = self.serial {
            write!(serial, "{:?}", args)?;
        }
        Ok(())
    }
}

#[inline]
fn write_level_vga(vga: &mut vga::Writer, level: &Level) -> fmt::Result {
    const DEFAULT_COLOR: vga::ColorSpec =
        vga::ColorSpec::new(vga::Color::LightGray, vga::Color::Black);
    const TRACE_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::Black, vga::Color::Magenta);
    const DEBUG_COLOR: vga::ColorSpec =
        vga::ColorSpec::new(vga::Color::Black, vga::Color::LightBlue);
    const INFO_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::Black, vga::Color::Green);
    const WARN_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::Black, vga::Color::Yellow);
    const ERR_COLOR: vga::ColorSpec = vga::ColorSpec::new(vga::Color::Black, vga::Color::Red);
    vga.set_color(DEFAULT_COLOR);
    match *level {
        Level::TRACE => {
            vga.set_color(TRACE_COLOR);
            vga.write_str("TRACE")?;
            vga.set_color(DEFAULT_COLOR);
        }
        Level::DEBUG => {
            vga.set_color(DEBUG_COLOR);
            vga.write_str("DEBUG")?;
            vga.set_color(DEFAULT_COLOR);
        }
        Level::INFO => {
            vga.set_color(INFO_COLOR);
            vga.write_str("INFO").unwrap();
            vga.set_color(DEFAULT_COLOR);
        }
        Level::WARN => {
            vga.set_color(WARN_COLOR);
            vga.write_str("WARN")?;
            vga.set_color(DEFAULT_COLOR);
        }
        Level::ERROR => {
            vga.set_color(ERR_COLOR);
            vga.write_str("ERROR")?;
            vga.set_color(DEFAULT_COLOR);
        }
    };

    vga.write_char(' ')?;
    Ok(())
}

#[inline]
fn write_level_serial(serial: &mut serial::Lock<'_>, level: &Level) -> fmt::Result {
    match *level {
        Level::TRACE => serial.write_str("[TRACE]")?,
        Level::DEBUG => serial.write_str("[DEBUG]")?,
        Level::INFO => serial.write_str(" [INFO]")?,
        Level::WARN => serial.write_str(" [WARN]")?,
        Level::ERROR => serial.write_str("[ERROR]")?,
    };

    Ok(())
}

impl<'a> Writer<'a> {
    fn indent_span(&mut self) -> fmt::Result {
        self.indent_vga()?;
        self.indent_serial(true)?;
        Ok(())
    }

    #[inline]
    fn indent_vga(&mut self) -> fmt::Result {
        if let Some((ref mut vga, indent)) = self.vga {
            for _ in 0..indent.load(Ordering::Relaxed) {
                vga.write_char(' ')?;
            }
        }
        Ok(())
    }

    #[inline]
    fn indent_serial(&mut self, span: bool) -> fmt::Result {
        if let Some((ref mut serial, indent)) = self.serial {
            let indent = indent.load(Ordering::Relaxed);
            if indent > 0 {
                for _ in 0..indent {
                    serial.write_str(" |")?;
                }
                if !span {
                    serial.write_char('-')?;
                }
            }
            serial.write_char(' ')?;
        }
        Ok(())
    }

    fn write_level(&mut self, level: &Level) -> fmt::Result {
        if let Some((ref mut vga, _t)) = self.vga {
            write_level_vga(vga, level)?;
        }

        if let Some((ref mut serial, _)) = self.serial {
            write_level_serial(serial, level)?;
        }

        Ok(())
    }
}

impl<'a, W: Write> field::Visit for Visitor<'a, W> {
    fn record_debug(&mut self, field: &field::Field, val: &dyn fmt::Debug) {
        if field.name() == "message" {
            if self.seen {
                let _ = write!(self.writer, ", {:?}", val);
            } else {
                let _ = write!(self.writer, "{:?}", val);
                self.seen = true;
            }
        } else if self.seen {
            let _ = write!(self.writer, ", {}={:?}", field, val);
        } else {
            let _ = write!(self.writer, "{}={:?}", field, val);
            self.seen = true;
        }
    }
}

impl tracing::Subscriber for Subscriber {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let level = metadata.level();
        self.vga_enabled(level) || self.serial_enabled(level)
    }

    fn new_span(&self, span: &span::Attributes) -> span::Id {
        let meta = span.metadata();
        let level = meta.level();
        let mut writer = self.writer(level);
        if let Some((ref mut serial, _)) = writer.serial {
            let _ = write_level_serial(serial, level);
        }
        let _ = writer.indent_span();
        let _ = write!(&mut writer, "{}", meta.name());
        {
            let mut visitor = Visitor {
                writer: &mut writer,
                seen: true,
            };
            span.record(&mut visitor);
        }

        let mut id = self.next_id.fetch_add(1, Ordering::Acquire);
        if id & SERIAL_BIT != 0 {
            // we have used a _lot_ of span IDs...presumably the low-numbered
            // spans are gone by now.
            self.next_id.store(0, Ordering::Release);
        }

        if self.vga_enabled(level) {
            // mark that this span should be written to the VGA buffer.
            id |= VGA_BIT;
        }

        if self.serial_enabled(level) {
            // mark that this span should be written to the serial port buffer.
            id |= SERIAL_BIT;
        }
        let _ = writer.write_str("\n");
        span::Id::from_u64(id)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record) {
        // nop for now
    }

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {
        // nop for now
    }

    fn event(&self, event: &Event) {
        let meta = event.metadata();
        let level = meta.level();
        let mut writer = self.writer(level);
        let _ = writer.indent_vga();
        let _ = writer.write_level(level);
        let _ = writer.indent_serial(false);

        if let Some((ref mut serial, _)) = writer.serial {
            let _ = write!(serial, "{}: ", meta.target());
        }
        {
            let mut visitor = Visitor {
                writer: &mut writer,
                seen: false,
            };
            event.record(&mut visitor);
        }
        let _ = writer.write_str("\n");
    }

    fn enter(&self, span: &span::Id) {
        let bits = span.into_u64();
        if bits & VGA_BIT != 0 {
            self.vga_indent.fetch_add(2, Ordering::Relaxed);
        }
        if bits & SERIAL_BIT != 0 {
            if let Some(Serial { ref indent, .. }) = self.serial {
                indent.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn exit(&self, span: &span::Id) {
        let bits = span.into_u64();
        if bits & VGA_BIT != 0 {
            self.vga_indent.fetch_sub(2, Ordering::Relaxed);
        }
        if bits & SERIAL_BIT != 0 {
            if let Some(Serial { ref indent, .. }) = self.serial {
                indent.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}
