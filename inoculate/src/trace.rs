use crate::{
    term::{style, ColorMode, OwoColorize},
    Options, Result,
};
use heck::TitleCase;
use std::fmt;
use tracing::{field::Field, Event, Level, Subscriber};
use tracing_subscriber::{
    field::Visit,
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields, FormattedFields},
    registry::LookupSpan,
};

pub fn try_init(opts: &Options) -> Result<()> {
    use tracing_subscriber::prelude::*;
    let fmt = tracing_subscriber::fmt::layer()
        .event_format(CargoFormatter::default())
        .with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(fmt)
        .with(tracing_error::ErrorLayer::default())
        .with(opts.log.parse::<tracing_subscriber::EnvFilter>()?)
        .try_init()?;
    Ok(())
}

#[derive(Default)]
struct CargoFormatter {
    colors: ColorMode,
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
        let color = self.colors.should_color_stderr();

        let include_spans = {
            let mut visitor = Visitor::new(*level, writer.by_ref(), color);
            event.record(&mut visitor);
            !visitor.did_cargo_format && ctx.lookup_current().is_some()
        };

        writer.write_char('\n')?;

        if include_spans {
            let pipe_style = if color {
                style().blue().bold()
            } else {
                style()
            };
            let span_name = if color { style().bold() } else { style() };
            writeln!(
                writer,
                "   {} {}",
                "-->".style(pipe_style),
                metadata.file().unwrap_or_else(|| metadata.target()),
            )?;
            ctx.visit_spans(|span| {
                let exts = span.extensions();
                let fields = exts
                    .get::<FormattedFields<N>>()
                    .map(|f| f.fields.as_str())
                    .unwrap_or("");
                writeln!(
                    writer,
                    "    {}  {}{}{}",
                    "|".style(pipe_style),
                    span.name().style(span_name),
                    if fields.is_empty() { "" } else { ": " },
                    fields
                )
            })?;

            writer.write_char('\n')?;
        }

        Ok(())
    }
}

struct Visitor<'writer> {
    level: Level,
    writer: Writer<'writer>,
    is_empty: bool,
    color: bool,
    did_cargo_format: bool,
}

impl<'writer> Visitor<'writer> {
    const MESSAGE: &'static str = "message";
    const INDENT: usize = 12;

    fn new(level: Level, writer: Writer<'writer>, color: bool) -> Self {
        Self {
            level,
            writer,
            is_empty: true,
            did_cargo_format: false,
            color,
        }
    }
}

impl Visit for Visitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if self.is_empty {
            if self.level >= Level::INFO && field.name() == Self::MESSAGE {
                let message = format!("{:?}", value);
                if let Some((tag, message)) = message.as_str().split_once(' ') {
                    if tag.len() <= Self::INDENT && tag.ends_with("ing") || tag.ends_with('d') {
                        let tag = tag.to_title_case();
                        let style = match (self.level, self.color) {
                            (Level::DEBUG, true) => style().bright_blue().bold(),
                            (_, true) => style().green().bold(),
                            (_, false) => style(),
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
                        return;
                    }
                }
            }

            let _ = match (self.level, self.color) {
                (Level::ERROR, true) => {
                    write!(self.writer, "{}{} ", "error".red().bold(), ":".bold())
                }
                (Level::WARN, true) => {
                    write!(self.writer, "{}{} ", "warning".yellow().bold(), ":".bold())
                }
                (Level::INFO, true) => {
                    write!(self.writer, "{}{} ", "info".green().bold(), ":".bold())
                }
                (Level::DEBUG, true) => {
                    write!(self.writer, "{}{} ", "debug".blue().bold(), ":".bold())
                }
                (Level::TRACE, true) => {
                    write!(self.writer, "{}{} ", "trace".purple().bold(), ":".bold())
                }
                (Level::ERROR, false) => self.writer.write_str("error: "),
                (Level::WARN, false) => self.writer.write_str("warning: "),
                (Level::INFO, false) => self.writer.write_str("info: "),
                (Level::DEBUG, false) => self.writer.write_str("debug: "),
                (Level::TRACE, false) => self.writer.write_str("trace: "),
            };
        }

        if !self.is_empty {
            let _ = self.writer.write_str(", ");
        }

        let bold = if self.color { style().bold() } else { style() };
        if field.name() == Self::MESSAGE {
            let _ = write!(self.writer, "{:?}", value.style(bold));
        } else {
            let _ = write!(
                self.writer,
                "{}{} {:?}",
                field.name().style(bold),
                ":".style(bold),
                value
            );
        }

        self.is_empty = false;
    }
}
