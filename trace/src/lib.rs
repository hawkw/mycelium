#![no_std]
pub mod writer;

use crate::writer::MakeWriter;
use core::sync::atomic::{AtomicU64, Ordering};
use mycelium_util::{
    fmt::{self, Write, WriteExt},
    io,
};
use tracing_core::{field, span, Event, Level, LevelFilter, Metadata};

#[derive(Debug)]
pub struct Subscriber<D, S = fn() -> io::Sink> {
    display: Output<D, VGA_BIT>,
    serial: Output<S, SERIAL_BIT>,
    next_id: AtomicU64,
}

#[derive(Debug)]
struct Output<W, const BIT: u64> {
    make_writer: W,
    max_level: LevelFilter,
    cfg: OutputCfg,
}

#[derive(Debug)]
struct OutputCfg {
    line_len: usize,
    indent: AtomicU64,
    span_indent_chars: &'static str,
}

#[derive(Debug)]
struct Writer<'a, W: io::Write> {
    cfg: &'a OutputCfg,
    current_line: usize,
    writer: W,
}

#[derive(Debug)]
struct WriterPair<'a, D: io::Write, S: io::Write> {
    display: Option<Writer<'a, D>>,
    serial: Option<Writer<'a, S>>,
}

struct Visitor<'writer, W> {
    writer: fmt::WithIndent<'writer, W>,
    seen: bool,
    newline: bool,
    comma: bool,
}

// === impl Subscriber ===

impl<D: Default> Default for Subscriber<D> {
    fn default() -> Self {
        Self::display_only(D::default())
    }
}

const SERIAL_BIT: u64 = 1 << 62;
const VGA_BIT: u64 = 1 << 63;
const _ACTUAL_ID_BITS: u64 = !(SERIAL_BIT | VGA_BIT);

impl<D> Subscriber<D> {
    pub fn display_only(display: D) -> Self {
        Self {
            display: Output {
                make_writer: display,
                max_level: LevelFilter::INFO,
                cfg: OutputCfg {
                    line_len: 80,
                    indent: AtomicU64::new(0),
                    span_indent_chars: " ",
                },
            },
            serial: Output {
                make_writer: io::sink,
                max_level: LevelFilter::OFF,
                cfg: OutputCfg {
                    line_len: 80,
                    indent: AtomicU64::new(0),
                    span_indent_chars: " |",
                },
            },
            next_id: AtomicU64::new(0),
        }
    }

    pub fn with_serial<S>(self, port: S) -> Subscriber<D, S>
    where
        for<'a> S: MakeWriter<'a>,
    {
        Subscriber {
            serial: Output {
                make_writer: port,
                max_level: LevelFilter::TRACE,
                cfg: self.serial.cfg,
            },
            display: self.display,
            next_id: self.next_id,
        }
    }
}

impl<D, S> Subscriber<D, S> {
    pub fn display_max_level(self, max_level: impl Into<LevelFilter>) -> Self {
        Self {
            display: Output {
                max_level: max_level.into(),
                ..self.display
            },
            ..self
        }
    }

    fn writer<'a>(&'a self, meta: &Metadata<'_>) -> WriterPair<'a, D::Writer, S::Writer>
    where
        D: MakeWriter<'a>,
        S: MakeWriter<'a>,
    {
        WriterPair {
            display: self.display.writer(meta),
            serial: self.serial.writer(meta),
        }
    }
}

impl<D, S> tracing_core::Subscriber for Subscriber<D, S>
where
    for<'a> D: MakeWriter<'a> + 'static,
    for<'a> S: MakeWriter<'a> + 'static,
{
    fn enabled(&self, meta: &Metadata) -> bool {
        self.display.enabled(meta) || self.serial.enabled(meta)
    }

    fn new_span(&self, span: &span::Attributes) -> span::Id {
        let meta = span.metadata();
        let mut writer = self.writer(meta);
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

        if self.display.enabled(meta) {
            // mark that this span should be written to the VGA buffer.
            id |= VGA_BIT;
        }

        if self.serial.enabled(meta) {
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
        let mut writer = self.writer(meta);
        let _ = write_level(&mut writer, meta.level());
        let _ = writer.indent(false);
        let _ = write!(writer, "{}: ", meta.target());
        event.record(&mut Visitor::new(&mut writer, meta.fields()));
    }

    fn enter(&self, span: &span::Id) {
        let bits = span.into_u64();
        self.display.enter(bits);
        self.serial.enter(bits);
    }

    fn exit(&self, span: &span::Id) {
        let bits = span.into_u64();
        self.display.exit(bits);
        self.serial.exit(bits);
    }
}

// === impl Output ===

impl<W, const BIT: u64> Output<W, BIT> {
    #[inline]
    fn enabled(&self, metadata: &tracing_core::Metadata<'_>) -> bool {
        metadata.level() <= &self.max_level
    }

    #[inline]
    fn enter(&self, id: u64) {
        if id & BIT != 0 {
            self.cfg.indent.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    fn exit(&self, id: u64) {
        if id & BIT != 0 {
            self.cfg.indent.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn writer<'a>(&'a self, meta: &Metadata<'_>) -> Option<Writer<'a, W::Writer>>
    where
        W: MakeWriter<'a>,
    {
        if self.enabled(meta) {
            return Some(Writer {
                current_line: 0,
                writer: self.make_writer.make_writer_for(meta),
                cfg: &self.cfg,
            });
        }

        None
    }
}

// === impl WriterPair ===

impl<'a, D, S> Write for WriterPair<'a, D, S>
where
    D: io::Write,
    S: io::Write,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
        use io::Write;
        if let Some(ref mut display) = self.display {
            display.write(s.as_bytes()).map_err(|_| fmt::Error)?;
        }
        if let Some(ref mut serial) = self.serial {
            serial.write(s.as_bytes()).map_err(|_| fmt::Error)?;
        }
        Ok(())
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        use io::Write;
        if let Some(ref mut display) = self.display {
            write!(display, "{:?}", args).map_err(|_| fmt::Error)?;
        }
        if let Some(ref mut serial) = self.serial {
            write!(serial, "{:?}", args).map_err(|_| fmt::Error)?;
        }
        Ok(())
    }
}

impl<'a, D, S> WriterPair<'a, D, S>
where
    D: io::Write,
    S: io::Write,
{
    fn indent(&mut self, is_span: bool) -> fmt::Result {
        use io::Write;
        if let Some(ref mut display) = self.display {
            display.indent().map_err(|_| fmt::Error)?;
        }
        if let Some(ref mut serial) = self.serial {
            serial.indent().map_err(|_| fmt::Error)?;
            let chars = if is_span { b"-" } else { b" " };

            serial.write(chars).map_err(|_| fmt::Error)?;
        }
        Ok(())
    }
}

// ==== impl Writer ===

impl<'a, W: io::Write> Writer<'a, W> {
    fn indent(&mut self) -> io::Result<()> {
        for _ in 0..self.cfg.indent.load(Ordering::Acquire) {
            self.writer
                .write_all(self.cfg.span_indent_chars.as_bytes())?;
            self.current_line += self.cfg.span_indent_chars.len();
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
            while self.current_line + line.len() >= self.cfg.line_len {
                let offset = if let Some(last_ws) = line[..self.cfg.line_len - self.current_line]
                    .iter()
                    .rev()
                    .position(|&c| c.is_ascii_whitespace())
                {
                    // found a nice whitespace to break on!
                    self.writer.write(&line[..last_ws])?;
                    last_ws
                } else {
                    0
                };
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
    fn new(writer: &'writer mut W, fields: &tracing_core::field::FieldSet) -> Self {
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
