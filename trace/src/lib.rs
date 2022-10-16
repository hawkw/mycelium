#![no_std]
#![feature(doc_cfg)]

#[cfg(feature = "embedded-graphics")]
#[doc(cfg(feature = "embedded-graphics"))]
pub mod embedded_graphics;
pub mod writer;

use crate::writer::MakeWriter;
use core::sync::atomic::{AtomicU64, Ordering};
use mycelium_util::fmt::{self, Write, WriteExt};
use tracing_core::{field, span, Event, Level, Metadata};

#[derive(Debug)]
pub struct Subscriber<D, S = Option<writer::NoWriter>> {
    display: Output<D, VGA_BIT>,
    serial: Output<S, SERIAL_BIT>,
    next_id: AtomicU64,
}

#[derive(Debug)]
struct Output<W, const BIT: u64> {
    make_writer: W,
    cfg: OutputCfg,
}

#[derive(Debug)]
struct OutputCfg {
    line_len: usize,
    indent: AtomicU64,
    span_indent_chars: &'static str,
}

#[derive(Debug)]
struct Writer<'a, W: Write> {
    cfg: &'a OutputCfg,
    current_line: usize,
    writer: W,
}

#[derive(Debug)]
struct WriterPair<'a, D: Write, S: Write> {
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

impl<D> Default for Subscriber<D>
where
    for<'a> D: MakeWriter<'a>,
    D: Default,
{
    fn default() -> Self {
        Self::display_only(D::default())
    }
}

const SERIAL_BIT: u64 = 1 << 62;
const VGA_BIT: u64 = 1 << 63;
const _ACTUAL_ID_BITS: u64 = !(SERIAL_BIT | VGA_BIT);

impl<D, S> Subscriber<D, S> {
    pub fn display_only(display: D) -> Self
    where
        for<'a> D: MakeWriter<'a>,
        for<'a> S: MakeWriter<'a>,
        S: Default,
    {
        Self {
            display: Output::new(display, " "),
            serial: Output::new(S::default(), " |"),
            next_id: AtomicU64::new(0),
        }
    }

    pub fn with_serial(self, port: S) -> Subscriber<D, S>
    where
        for<'a> S: MakeWriter<'a>,
    {
        Subscriber {
            serial: Output::new(port, " |"),
            display: self.display,
            next_id: self.next_id,
        }
    }
}

impl<D, S> Subscriber<D, S> {
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

impl<D, S> tracing_core::Collect for Subscriber<D, S>
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
        let _ = write!(writer, "{}: ", meta.name());

        span.record(&mut Visitor::new(&mut writer));

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
        event.record(&mut Visitor::new(&mut writer));
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

    fn current_span(&self) -> tracing_core::span::Current {
        // TODO(eliza): fix
        tracing_core::span::Current::none()
    }
}

// === impl Output ===

impl<W, const BIT: u64> Output<W, BIT> {
    fn new<'a>(make_writer: W, span_indent_chars: &'static str) -> Self
    where
        W: MakeWriter<'a>,
    {
        let cfg = OutputCfg {
            line_len: make_writer.line_len(),
            indent: AtomicU64::new(0),
            span_indent_chars,
        };
        Self { make_writer, cfg }
    }

    #[inline]
    fn enabled<'a>(&'a self, metadata: &tracing_core::Metadata<'_>) -> bool
    where
        W: MakeWriter<'a>,
    {
        self.make_writer.enabled(metadata)
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
        let writer = self.make_writer.make_writer_for(meta)?;
        Some(Writer {
            current_line: 0,
            writer,
            cfg: &self.cfg,
        })
    }
}

// === impl WriterPair ===

impl<'a, D, S> Write for WriterPair<'a, D, S>
where
    D: Write,
    S: Write,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let err = if let Some(ref mut display) = self.display {
            display.write_str(s)
        } else {
            Ok(())
        };
        if let Some(ref mut serial) = self.serial {
            serial.write_str(s)?;
        }
        err
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        let err = if let Some(ref mut display) = self.display {
            display.write_fmt(args)
        } else {
            Ok(())
        };
        if let Some(ref mut serial) = self.serial {
            serial.write_fmt(args)?;
        }
        err
    }
}

impl<'a, D, S> WriterPair<'a, D, S>
where
    D: Write,
    S: Write,
{
    fn indent(&mut self, is_span: bool) -> fmt::Result {
        let err = if let Some(ref mut display) = self.display {
            // "rust has try-catch syntax lol"
            (|| {
                display.indent()?;
                if is_span {
                    display.write_char(' ')?;
                };
                Ok(())
            })()
        } else {
            Ok(())
        };
        if let Some(ref mut serial) = self.serial {
            serial.indent()?;
            let chars = if is_span { '-' } else { ' ' };

            serial.write_char(chars)?;
        }
        err
    }
}

// ==== impl Writer ===

impl<'a, W: Write> Writer<'a, W> {
    fn indent(&mut self) -> fmt::Result {
        for _ in 0..self.cfg.indent.load(Ordering::Acquire) {
            self.writer.write_str(self.cfg.span_indent_chars)?;
            self.current_line += self.cfg.span_indent_chars.len();
        }
        Ok(())
    }

    fn write_newline(&mut self) -> fmt::Result {
        self.writer.write_str("   ")?;
        self.current_line = 3;
        self.indent()
    }

    fn finish(&mut self) -> fmt::Result {
        self.writer.write_char('\n')
    }
}

impl<'a, W> Write for Writer<'a, W>
where
    W: Write,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let lines = s.split_inclusive('\n');
        for line in lines {
            let mut line = line;
            while self.current_line + line.len() >= self.cfg.line_len {
                let offset = if let Some(last_ws) = line[..self.cfg.line_len - self.current_line]
                    .chars()
                    .rev()
                    .position(|c| c.is_whitespace())
                {
                    // found a nice whitespace to break on!
                    self.writer.write_str(&line[..last_ws])?;
                    last_ws
                } else {
                    0
                };
                self.writer.write_char('\n')?;
                self.write_newline()?;
                self.writer.write_str("  ")?;
                self.current_line += 2;
                line = &line[offset..];
            }
            self.writer.write_str(line)?;
            if line.ends_with('\n') {
                self.write_newline()?;
                self.writer.write_char(' ')?;
            }
            self.current_line += line.len();
        }

        Ok(())
    }

    fn write_char(&mut self, ch: char) -> fmt::Result {
        self.writer.write_char(ch)?;
        if ch == '\n' {
            self.write_newline()
        } else {
            Ok(())
        }
    }
}

impl<'a, W: Write> Drop for Writer<'a, W> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[inline]
fn write_level(w: &mut impl fmt::Write, level: &Level) -> fmt::Result {
    match *level {
        Level::TRACE => w.write_str("[*]")?,
        Level::DEBUG => w.write_str("[?]")?,
        Level::INFO => w.write_str("[i]")?,
        Level::WARN => w.write_str("[!]")?,
        Level::ERROR => w.write_str("[x]")?,
    };

    Ok(())
}

impl<'writer, W: fmt::Write> Visitor<'writer, W> {
    fn new(writer: &'writer mut W) -> Self {
        Self {
            writer: writer.with_indent(2),
            seen: false,
            comma: false,
            newline: false,
        }
    }

    fn record_inner(&mut self, field: &field::Field, val: &dyn fmt::Debug) {
        // XXX(eliza): sad and gross hack
        struct HasWrittenNewline<'a, W> {
            writer: &'a mut W,
            has_written_newline: bool,
            has_written_punct: bool,
        }

        impl<'a, W: fmt::Write> fmt::Write for HasWrittenNewline<'a, W> {
            #[inline]
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.has_written_punct = s.ends_with(|ch: char| ch.is_ascii_punctuation());
                if s.contains('\n') {
                    self.has_written_newline = true;
                }
                self.writer.write_str(s)
            }
        }

        let mut writer = HasWrittenNewline {
            writer: &mut self.writer,
            has_written_newline: false,
            has_written_punct: false,
        };
        let nl = if self.newline { '\n' } else { ' ' };

        if field.name() == "message" {
            if self.seen {
                let _ = write!(writer, ",{}{:?}", nl, val);
            } else {
                let _ = write!(writer, "{:?}", val);
                self.comma = !writer.has_written_punct;
            }
            self.seen = true;
            return;
        }

        if self.comma {
            let _ = writer.write_char(',');
        }

        if self.seen {
            let _ = writer.write_char(nl);
        }

        if !self.comma {
            self.seen = true;
            self.comma = true;
        }

        let _ = write!(writer, "{}={:?}", field, val);
        self.newline |= writer.has_written_newline;
    }
}

impl<'writer, W: fmt::Write> field::Visit for Visitor<'writer, W> {
    #[inline]
    fn record_u64(&mut self, field: &field::Field, val: u64) {
        self.record_inner(field, &val)
    }

    #[inline]
    fn record_i64(&mut self, field: &field::Field, val: i64) {
        self.record_inner(field, &val)
    }

    #[inline]
    fn record_bool(&mut self, field: &field::Field, val: bool) {
        self.record_inner(field, &val)
    }

    #[inline]
    fn record_str(&mut self, field: &field::Field, val: &str) {
        if val.len() >= 70 {
            self.newline = true;
        }
        self.record_inner(field, &val)
    }

    fn record_debug(&mut self, field: &field::Field, val: &dyn fmt::Debug) {
        self.record_inner(field, &fmt::alt(val))
    }
}
