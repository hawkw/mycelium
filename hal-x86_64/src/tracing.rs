use crate::{serial, vga};
use core::sync::atomic::{AtomicU64, Ordering};
use mycelium_util::{
    fmt::{self, Write, WriteExt},
    io,
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

    fn writer(&self, level: &Level) -> WriterPair<'_> {
        let vga = None;
        let serial = if self.serial_enabled(level) {
            self.serial
                .as_ref()
                .map(|s| Writer::new(s.port.lock(), &s.indent, " |"))
        } else {
            None
        };
        WriterPair { vga, serial }
    }
}

struct Writer<'a, W: io::Write> {
    line_len: usize,
    current_line: usize,
    span_indent_chars: &'static str,
    span_indent: &'a AtomicU64,
    writer: W,
}

struct WriterPair<'a> {
    vga: Option<Writer<'a, vga::Writer>>,
    serial: Option<Writer<'a, serial::Lock<'a>>>,
}

struct Visitor<'writer, W> {
    writer: fmt::WithIndent<'writer, W>,
    seen: bool,
    newline: bool,
    comma: bool,
}

impl<'a> Write for WriterPair<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        use io::Write;
        if let Some(ref mut vga) = self.vga {
            vga.write(s.as_bytes()).map_err(|_| fmt::Error)?;
        }
        if let Some(ref mut serial) = self.serial {
            serial.write(s.as_bytes()).map_err(|_| fmt::Error)?;
        }
        Ok(())
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        use io::Write;
        if let Some(ref mut vga) = self.vga {
            write!(vga, "{:?}", args).map_err(|_| fmt::Error)?;
        }
        if let Some(ref mut serial) = self.serial {
            write!(serial, "{:?}", args).map_err(|_| fmt::Error)?;
        }
        Ok(())
    }
}

impl<'a> WriterPair<'a> {
    fn indent(&mut self, is_span: bool) -> fmt::Result {
        use io::Write;
        if let Some(ref mut vga) = self.vga {
            vga.indent().map_err(|_| fmt::Error)?;
        }
        if let Some(ref mut serial) = self.serial {
            serial.indent().map_err(|_| fmt::Error)?;
            let chars = if is_span { b"-" } else { b" " };

            serial.write(chars).map_err(|_| fmt::Error)?;
        }
        Ok(())
    }
}

impl<'a, W: io::Write> Writer<'a, W> {
    fn new(writer: W, indent: &'a AtomicU64, indent_chars: &'static str) -> Self {
        Self {
            current_line: 0,
            line_len: 80,
            span_indent: indent,
            writer,
            span_indent_chars: indent_chars,
        }
    }

    fn indent(&mut self) -> io::Result<()> {
        for _ in 0..self.span_indent.load(Ordering::Acquire) {
            self.writer.write_all(self.span_indent_chars.as_bytes())?;
            self.current_line += self.span_indent_chars.len();
        }
        Ok(())
    }

    fn write_newline(&mut self) -> io::Result<()> {
        self.writer.write_all(b"   ")?;
        self.current_line = 3;
        self.indent()
    }

    fn finish(&mut self) -> io::Result<()> {
        self.writer.write(b"\n")?;
        self.writer.flush()
    }
}

impl<'a, W> io::Write for Writer<'a, W>
where
    W: io::Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_all(buf)?;
        Ok(buf.len())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let lines = buf.split_inclusive(|&c| c == b'\n');
        for line in lines {
            let mut line = line;
            while self.current_line + line.len() >= self.line_len {
                let mut offset = self.line_len - self.current_line;
                if let Some(last_ws) = &line[..offset]
                    .iter()
                    .rev()
                    .position(|&c| c.is_ascii_whitespace())
                {
                    offset = *last_ws;
                }
                self.writer.write(&line[..offset])?;
                self.writer.write(b"\n")?;
                self.write_newline()?;
                self.writer.write(b"  ")?;
                self.current_line += 2;
                line = &line[offset..];
            }
            let wrote = self.writer.write(line)?;
            if line.last().map(|x| x == &b'\n').unwrap_or(false) {
                self.write_newline()?;
                self.writer.write(b" ")?;
            }
            self.current_line += wrote;
        }

        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<'a, W: io::Write> Drop for Writer<'a, W> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[inline]
fn write_level(w: &mut impl fmt::Write, level: &Level) -> fmt::Result {
    match *level {
        Level::TRACE => w.write_str("[+]")?,
        Level::DEBUG => w.write_str("[-]")?,
        Level::INFO => w.write_str("[*]")?,
        Level::WARN => w.write_str("[!]")?,
        Level::ERROR => w.write_str("[x]")?,
    };

    Ok(())
}

impl<'writer, W: fmt::Write> Visitor<'writer, W> {
    fn new(writer: &'writer mut W, fields: &tracing::field::FieldSet) -> Self {
        Self {
            writer: writer.with_indent(2),
            seen: false,
            comma: false,
            newline: fields.iter().filter(|f| f.name() != "message").count() > 1,
        }
    }

    fn record_inner(&mut self, field: &field::Field, val: &dyn fmt::Debug, nl: &'static str) {
        if field.name() == "message" {
            if self.seen {
                let _ = write!(self.writer, ",{}{:?}", nl, val);
            } else {
                let _ = write!(self.writer, "{:?}", val);
            }
            self.seen = true;
            return;
        }

        if self.comma {
            let _ = self.writer.write_char(',');
        } else {
            self.seen = true;
            self.comma = true;
        }

        let _ = write!(self.writer, "{}{}={:?}", nl, field, val);
    }
}

impl<'writer, W: fmt::Write> field::Visit for Visitor<'writer, W> {
    fn record_u64(&mut self, field: &field::Field, val: u64) {
        let nl = if self.newline { "\n" } else { " " };
        self.record_inner(field, &val, nl)
    }

    fn record_i64(&mut self, field: &field::Field, val: i64) {
        let nl = if self.newline { "\n" } else { " " };
        self.record_inner(field, &val, nl)
    }

    fn record_bool(&mut self, field: &field::Field, val: bool) {
        let nl = if self.newline { "\n" } else { " " };
        self.record_inner(field, &val, nl)
    }

    fn record_str(&mut self, field: &field::Field, val: &str) {
        let nl = if self.newline || val.len() > 70 {
            "\n"
        } else {
            " "
        };
        self.record_inner(field, &val, nl)
    }

    fn record_debug(&mut self, field: &field::Field, val: &dyn fmt::Debug) {
        let nl = if self.newline || self.seen { "\n" } else { " " };
        self.record_inner(field, &fmt::alt(val), nl)
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
        // if let Some((ref mut serial, _)) = writer.serial {
        //     let _ = write_level_serial(serial, level);
        // }
        let _ = write_level(&mut writer, meta.level());
        let _ = writer.indent(true);
        let _ = write!(&mut writer, "{}", meta.name());

        span.record(&mut Visitor::new(&mut writer, meta.fields()));

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
        // let _ = writer.indent_vga();
        let _ = write_level(&mut writer, meta.level());
        let _ = writer.indent(false);
        let _ = write!(writer, "{}: ", meta.target());
        event.record(&mut Visitor::new(&mut writer, meta.fields()));
    }

    fn enter(&self, span: &span::Id) {
        let bits = span.into_u64();
        if bits & VGA_BIT != 0 {
            self.vga_indent.fetch_add(1, Ordering::Relaxed);
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
