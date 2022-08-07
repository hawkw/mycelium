use crate::term::{style, ColorMode, OwoColorize, Style};
use heck::TitleCase;
use std::fmt;
use tracing::{field::Field, Event, Level, Subscriber};
use tracing_subscriber::{
    field::Visit,
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields, FormattedFields},
    registry::LookupSpan,
};

#[derive(Debug)]
pub(crate) struct CargoFormatter {
    styles: Styles,
}

struct Visitor<'styles, 'writer> {
    level: Level,
    writer: Writer<'writer>,
    is_empty: bool,
    styles: &'styles Styles,
    skip_cargo_format: bool,
    did_cargo_format: bool,
    needs_comma: bool,
    log_target: Option<String>,
}

#[derive(Debug)]
struct Styles {
    error: Style,
    warn: Style,
    info: Style,
    debug: Style,
    trace: Style,
    pipes: Style,
    bold: Style,
}

struct Prefixed<T> {
    prefix: &'static str,
    val: T,
}

impl<S, N> FormatEvent<S, N> for CargoFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        let level = metadata.level();
        let skip_tag = if *level == Level::INFO {
            metadata
                .fields()
                .iter()
                .any(|field| field.name() == SKIP_CARGO_FORMAT)
        } else {
            false
        };

        let (include_spans, log_target) = {
            let mut visitor = self.visitor(*level, writer.by_ref(), skip_tag);
            event.record(&mut visitor);
            let include_spans = !visitor.did_cargo_format && ctx.lookup_current().is_some();
            (include_spans, visitor.log_target)
        };

        writer.write_char('\n')?;

        if include_spans {
            writeln!(
                writer,
                "   {} {}{}",
                "-->".style(self.styles.pipes),
                log_target
                    .as_deref()
                    .or_else(|| metadata.file())
                    .unwrap_or_else(|| metadata.target()),
                DisplayOpt(metadata.line().map(Prefixed::prefix(":"))),
            )?;
            ctx.visit_spans(|span| {
                let exts = span.extensions();
                let fields = exts
                    .get::<FormattedFields<N>>()
                    .map(|f| f.fields.as_str())
                    .unwrap_or("");
                writeln!(
                    writer,
                    "    {} {}{}{}",
                    "|".style(self.styles.pipes),
                    span.name().style(self.styles.bold),
                    if fields.is_empty() { "" } else { ": " },
                    fields
                )
            })?;

            writer.write_char('\n')?;
        }

        Ok(())
    }
}
const SKIP_CARGO_FORMAT: &str = "cargo.skip_tag";
const LOG_FIELD_PREFIX: &str = "log.";

impl CargoFormatter {
    pub(crate) fn new(colors: ColorMode) -> Self {
        Self {
            styles: Styles::new(colors),
        }
    }

    fn visitor<'styles, 'writer>(
        &'styles self,
        level: Level,
        writer: Writer<'writer>,
        skip_cargo_format: bool,
    ) -> Visitor<'styles, 'writer> {
        Visitor {
            level,
            writer,
            is_empty: true,
            styles: &self.styles,
            skip_cargo_format,
            did_cargo_format: false,
            needs_comma: false,
            log_target: None,
        }
    }
}

// === impl Visitor ===

impl<'styles, 'writer> Visitor<'styles, 'writer> {
    const MESSAGE: &'static str = "message";
    const INDENT: usize = 12;
}

impl<'styles, 'writer> Visit for Visitor<'styles, 'writer> {
    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        if name.starts_with(LOG_FIELD_PREFIX) {
            if name == "log.file" || (name == "log.target" && self.log_target.is_none()) {
                self.log_target = Some(value.to_string());
            }
            return;
        }

        self.record_debug(field, &value)
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let name = field.name();

        // skip `log` metadata fields
        if name.starts_with(LOG_FIELD_PREFIX) {
            return;
        }

        // skip `cargo.skip_tag` fields, this is a special marker to control formatting
        if name == SKIP_CARGO_FORMAT {
            return;
        }

        // If we're writing the first field of the event, either emit cargo
        // formatting, or a level header.
        if self.is_empty {
            // If the level is `INFO` and it has a message that's
            // shaped like a cargo log tag, emit the cargo tag followed by the
            // rest of the message.
            if self.level == Level::INFO && field.name() == Self::MESSAGE && !self.skip_cargo_format
            {
                let message = format!("{:?}", value);
                if let Some((tag, message)) = message.as_str().split_once(' ') {
                    if tag.len() <= Self::INDENT {
                        let tag = tag.to_title_case();
                        let style = match self.level {
                            Level::DEBUG => self.styles.debug,
                            _ => self.styles.info,
                        };

                        let _ = write!(
                            self.writer,
                            "{:>indent$} ",
                            tag.style(style),
                            indent = Self::INDENT
                        );

                        let _ = self.writer.write_str(message);
                        self.is_empty = false;
                        self.did_cargo_format = true;
                        self.needs_comma = !message.ends_with(|ch: char| ch.is_ascii_punctuation());
                        return;
                    }
                }
            }

            // Otherwise, emit a level tag.
            let _ = match self.level {
                Level::ERROR => write!(
                    self.writer,
                    "{}{} ",
                    "error".style(self.styles.error),
                    ":".style(self.styles.bold)
                ),
                Level::WARN => write!(
                    self.writer,
                    "{}{} ",
                    "warning".style(self.styles.warn),
                    ":".style(self.styles.bold),
                ),
                Level::INFO => write!(
                    self.writer,
                    "{}{} ",
                    "info".style(self.styles.info),
                    ":".style(self.styles.bold)
                ),
                Level::DEBUG => write!(
                    self.writer,
                    "{}{} ",
                    "debug".style(self.styles.debug),
                    ":".style(self.styles.bold)
                ),
                Level::TRACE => write!(
                    self.writer,
                    "{}{} ",
                    "trace".style(self.styles.trace),
                    ":".style(self.styles.bold)
                ),
            };
        } else if self.needs_comma {
            // If this is *not* the first field of the event, prefix it with a
            // comma for the preceding field, instead of a cargo tag or level tag.
            let _ = self.writer.write_str(", ");
        } else {
            let _ = self.writer.write_char(' ');
        }

        if name == Self::MESSAGE {
            let _ = write!(self.writer, "{:?}", value.style(self.styles.bold));
        } else {
            let _ = write!(
                self.writer,
                "{}{} {:?}",
                field.name().style(self.styles.bold),
                ":".style(self.styles.bold),
                value
            );
        }

        self.is_empty = false;
        self.needs_comma = true;
    }
}

// === impl Styles ===

impl Styles {
    fn new(colors: ColorMode) -> Self {
        Self {
            error: colors.if_color(style().red().bold()),
            warn: colors.if_color(style().yellow().bold()),
            info: colors.if_color(style().green().bold()),
            debug: colors.if_color(style().blue().bold()),
            trace: colors.if_color(style().purple().bold()),
            bold: colors.if_color(style().bold()),
            pipes: colors.if_color(style().blue().bold()),
        }
    }
}

impl<T> Prefixed<T> {
    fn prefix(prefix: &'static str) -> impl Fn(T) -> Prefixed<T> {
        move |val| Prefixed { val, prefix }
    }
}

impl<T> fmt::Display for Prefixed<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.prefix, self.val)
    }
}

impl<T> fmt::Debug for Prefixed<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{:?}", self.prefix, self.val)
    }
}

struct DisplayOpt<T>(Option<T>);

impl<T> fmt::Display for DisplayOpt<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref val) = self.0 {
            fmt::Display::fmt(val, f)?;
        }

        Ok(())
    }
}
