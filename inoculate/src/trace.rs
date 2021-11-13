use crate::term::{style, ColorMode, OwoColorize};
use heck::TitleCase;
use std::fmt;
use tracing::{field::Field, Event, Level, Subscriber};
use tracing_subscriber::{
    field::Visit,
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields},
    registry::LookupSpan,
};

#[derive(Default)]
pub struct CargoFormatter(());

impl<S, N> FormatEvent<S, N> for CargoFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let level = event.metadata().level();

        let mut visitor = Visitor::new(*level, writer.by_ref());
        event.record(&mut visitor);

        writer.write_char('\n')?;

        Ok(())
    }
}

struct Visitor<'writer> {
    level: Level,
    writer: Writer<'writer>,
    is_empty: bool,
    color: bool,
}

impl<'writer> Visitor<'writer> {
    const MESSAGE: &'static str = "message";
    const INDENT: usize = 12;

    fn new(level: Level, writer: Writer<'writer>) -> Self {
        Self {
            level,
            writer,
            is_empty: true,
            color: ColorMode::default().should_color_stdout(),
        }
    }
}

impl Visit for Visitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if self.is_empty {
            if matches!(self.level, Level::INFO | Level::DEBUG) && field.name() == Self::MESSAGE {
                let message = format!("{:?}", value);
                if let Some((tag, message)) = message.as_str().split_once(' ') {
                    let tag = tag.to_title_case();
                    if tag.len() <= Self::INDENT {
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
                (Level::TRACE, false) => self.writer.write_str("TRACE: "),
            };
        }

        if !self.is_empty {
            let _ = self.writer.write_str(", ");
        }

        if field.name() == Self::MESSAGE {
            let _ = write!(self.writer, "{:?}", value);
        } else {
            let bold = if self.color { style().bold() } else { style() };
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
