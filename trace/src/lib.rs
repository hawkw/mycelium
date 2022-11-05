#![cfg_attr(not(test), no_std)]
#![feature(doc_cfg, doc_auto_cfg)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
pub mod buf;
pub mod color;
#[cfg(feature = "embedded-graphics")]
pub mod embedded_graphics;
pub mod writer;

use crate::{
    color::{Color, SetColor},
    writer::MakeWriter,
};
use core::sync::atomic::{AtomicU64, Ordering};
use mycelium_util::fmt::{self, Write};
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
    indent_cfg: IndentCfg,
}

#[derive(Debug, Copy, Clone)]
struct IndentCfg {
    event: &'static str,
    indent: &'static str,
    new_span: &'static str,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum IndentKind {
    Event,
    NewSpan,
    Indent,
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
    writer: &'writer mut W,
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

const SERIAL_BIT: u64 = 1 << 0;
const VGA_BIT: u64 = 1 << 1;
const _ACTUAL_ID_BITS: u64 = !(SERIAL_BIT | VGA_BIT);

impl<D, S> Subscriber<D, S> {
    pub fn display_only(display: D) -> Self
    where
        for<'a> D: MakeWriter<'a>,
        for<'a> S: MakeWriter<'a>,
        S: Default,
    {
        Self {
            display: Output::new(display, Self::DISPLAY_INDENT_CFG),
            serial: Output::new(S::default(), Self::SERIAL_INDENT_CFG),
            next_id: AtomicU64::new(0),
        }
    }

    pub fn with_serial(self, port: S) -> Subscriber<D, S>
    where
        for<'a> S: MakeWriter<'a>,
    {
        Subscriber {
            serial: Output::new(port, Self::SERIAL_INDENT_CFG),
            display: self.display,
            next_id: self.next_id,
        }
    }

    const SERIAL_INDENT_CFG: IndentCfg = IndentCfg {
        indent: "│",
        new_span: "┌",
        event: "├",
    };
    const DISPLAY_INDENT_CFG: IndentCfg = IndentCfg {
        indent: " ",
        new_span: "> ",
        event: " ",
    };
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

impl<D, S, DW, SW> tracing_core::Collect for Subscriber<D, S>
where
    for<'a> D: MakeWriter<'a, Writer = DW> + 'static,
    DW: Write + SetColor,
    for<'a> S: MakeWriter<'a, Writer = SW> + 'static,
    SW: Write + SetColor,
{
    fn enabled(&self, meta: &Metadata) -> bool {
        self.display.enabled(meta) || self.serial.enabled(meta)
    }

    fn new_span(&self, span: &span::Attributes) -> span::Id {
        let meta = span.metadata();
        let mut writer = self.writer(meta);
        let _ = write_level(&mut writer, meta.level());
        let _ = writer.indent_initial(IndentKind::NewSpan);
        let _ = writer.with_bold().write_str(meta.name());
        let _ = writer.with_fg_color(Color::BrightBlack).write_str(": ");

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
        let _ = writer.indent_initial(IndentKind::Event);
        let _ = write!(
            writer.with_fg_color(Color::BrightBlack),
            "{}: ",
            meta.target()
        );
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
    fn new<'a>(make_writer: W, indent_cfg: IndentCfg) -> Self
    where
        W: MakeWriter<'a>,
    {
        let cfg = OutputCfg {
            line_len: make_writer.line_len(),
            indent: AtomicU64::new(0),
            indent_cfg,
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
            self.cfg.indent.fetch_add(1, Ordering::Release);
        }
    }

    #[inline]
    fn exit(&self, id: u64) {
        if id & BIT != 0 {
            self.cfg.indent.fetch_sub(1, Ordering::Release);
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

impl<'a, D, S> SetColor for WriterPair<'a, D, S>
where
    D: Write + SetColor,
    S: Write + SetColor,
{
    fn set_fg_color(&mut self, color: Color) {
        if let Some(ref mut w) = self.display {
            w.set_fg_color(color)
        }
        if let Some(ref mut w) = self.serial {
            w.set_fg_color(color)
        };
    }

    fn fg_color(&self) -> Color {
        self.display
            .as_ref()
            .map(SetColor::fg_color)
            .or_else(|| self.serial.as_ref().map(SetColor::fg_color))
            .unwrap_or(Color::Default)
    }

    fn set_bold(&mut self, bold: bool) {
        if let Some(ref mut w) = self.display {
            w.set_bold(bold)
        }
        if let Some(ref mut w) = self.serial {
            w.set_bold(bold)
        };
    }
}

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
    fn indent_initial(&mut self, kind: IndentKind) -> fmt::Result {
        let err = if let Some(ref mut display) = self.display {
            // "rust has try-catch syntax lol"
            (|| {
                display.indent(kind)?;
                Ok(())
            })()
        } else {
            Ok(())
        };
        if let Some(ref mut serial) = self.serial {
            serial.indent(kind)?;
        }
        err
    }
}

// ==== impl Writer ===

impl<'a, W: Write> Writer<'a, W> {
    fn indent(&mut self, kind: IndentKind) -> fmt::Result {
        let indent = self.cfg.indent.load(Ordering::Acquire);
        self.write_indent(" ")?;

        for i in 1..=indent {
            let indent_str = match (i, kind) {
                (i, IndentKind::Event) if i == indent => self.cfg.indent_cfg.event,
                _ => self.cfg.indent_cfg.indent,
            };
            self.write_indent(indent_str)?;
        }

        if kind == IndentKind::NewSpan {
            self.write_indent(self.cfg.indent_cfg.new_span)?;
        }

        Ok(())
    }

    fn write_indent(&mut self, chars: &'static str) -> fmt::Result {
        self.writer.write_str(chars)?;
        self.current_line += chars.len();
        Ok(())
    }

    fn write_newline(&mut self) -> fmt::Result {
        self.writer.write_str("   ")?;
        self.current_line = 3;
        self.indent(IndentKind::Indent)
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
                self.writer.write_str(" ")?;
                self.current_line += 1;
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

impl<'a, W> SetColor for Writer<'a, W>
where
    W: Write + SetColor,
{
    fn fg_color(&self) -> Color {
        self.writer.fg_color()
    }

    fn set_fg_color(&mut self, color: Color) {
        self.writer.set_fg_color(color);
    }

    fn set_bold(&mut self, bold: bool) {
        self.writer.set_bold(bold)
    }
}

impl<'a, W: Write> Drop for Writer<'a, W> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[inline]
fn write_level<W>(w: &mut W, level: &Level) -> fmt::Result
where
    W: fmt::Write + SetColor,
{
    w.write_char('[')?;
    match *level {
        Level::TRACE => w.with_fg_color(Color::BrightBlue).write_char('*'),
        Level::DEBUG => w.with_fg_color(Color::BrightCyan).write_char('?'),
        Level::INFO => w.with_fg_color(Color::BrightGreen).write_char('i'),
        Level::WARN => w.with_fg_color(Color::BrightYellow).write_char('!'),
        Level::ERROR => w.with_fg_color(Color::BrightRed).write_char('x'),
    }?;
    w.write_char(']')
}

impl<'writer, W> Visitor<'writer, W>
where
    W: fmt::Write,
    &'writer mut W: SetColor,
{
    fn new(writer: &'writer mut W) -> Self {
        Self {
            writer,
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

        impl<'a, W: fmt::Write> SetColor for HasWrittenNewline<'a, W>
        where
            W: SetColor,
        {
            fn fg_color(&self) -> Color {
                self.writer.fg_color()
            }

            fn set_fg_color(&mut self, color: Color) {
                self.writer.set_fg_color(color);
            }

            fn set_bold(&mut self, bold: bool) {
                self.writer.set_bold(bold)
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
                let _ = write!(writer.with_bold(), "{}{:?}", nl, val);
            } else {
                let _ = write!(writer.with_bold(), "{:?}", val);
                self.comma = !writer.has_written_punct;
            }
            self.seen = true;
            return;
        }

        if self.comma {
            let _ = writer.with_fg_color(Color::BrightBlack).write_char(',');
        }

        if self.seen {
            let _ = writer.write_char(nl);
        }

        if !self.comma {
            self.seen = true;
            self.comma = true;
        }

        // pretty-print the name with dots in the punctuation color
        let mut name_pieces = field.name().split('.');
        if let Some(piece) = name_pieces.next() {
            let _ = writer.write_str(piece);
            for piece in name_pieces {
                let _ = writer.with_fg_color(Color::BrightBlack).write_char('.');
                let _ = writer.write_str(piece);
            }
        }

        let _ = writer.with_fg_color(Color::BrightBlack).write_char('=');
        let _ = write!(writer, "{val:?}");
        self.newline |= writer.has_written_newline;
    }
}

impl<'writer, W> field::Visit for Visitor<'writer, W>
where
    W: fmt::Write,
    &'writer mut W: SetColor,
{
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
