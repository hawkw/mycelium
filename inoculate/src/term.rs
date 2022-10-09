use color_eyre::eyre::format_err;
use std::{
    fmt,
    str::FromStr,
    sync::atomic::{AtomicU8, Ordering},
};

pub const CARGO_LOG_WIDTH: usize = 12;
pub use owo_colors::{style, OwoColorize, Style};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ColorMode {
    Auto = 0,
    Always = 1,
    Never = 2,
}

// === impl ColorMode ===

static GLOBAL_COLOR_MODE: AtomicU8 = AtomicU8::new(0);

impl FromStr for ColorMode {
    type Err = color_eyre::Report;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            s if s.eq_ignore_ascii_case("auto") => Ok(ColorMode::Auto),
            s if s.eq_ignore_ascii_case("always") => Ok(ColorMode::Always),
            s if s.eq_ignore_ascii_case("never") => Ok(ColorMode::Never),
            _ => Err(format_err!(
                "invalid color mode {:?}, expected one of \"auto\", \"always\", or \"never\"",
                s
            )),
        }
    }
}

impl fmt::Display for ColorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl ColorMode {
    pub fn if_color(self, style: owo_colors::Style) -> owo_colors::Style {
        if self.should_color_stderr() {
            style
        } else {
            owo_colors::style()
        }
    }

    pub fn set_global(self) {
        GLOBAL_COLOR_MODE
            .compare_exchange(0, self as u8, Ordering::AcqRel, Ordering::Acquire)
            .expect("global color mode already set");
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ColorMode::Auto => "auto",
            ColorMode::Always => "always",
            ColorMode::Never => "never",
        }
    }

    pub fn should_color_stdout(self) -> bool {
        match self {
            ColorMode::Auto => atty::is(atty::Stream::Stdout),
            ColorMode::Always => true,
            ColorMode::Never => false,
        }
    }

    pub fn should_color_stderr(self) -> bool {
        match self {
            ColorMode::Auto => atty::is(atty::Stream::Stderr),
            ColorMode::Always => true,
            ColorMode::Never => false,
        }
    }
}

impl Default for ColorMode {
    fn default() -> Self {
        match GLOBAL_COLOR_MODE.load(Ordering::Acquire) {
            1 => Self::Always,
            2 => Self::Never,
            _x => {
                debug_assert_eq!(_x, 0, "weird color mode, what the heck?");
                Self::Auto
            }
        }
    }
}
