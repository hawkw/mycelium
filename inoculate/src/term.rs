use clap::{ArgGroup, Args};
use std::{
    fmt,
    sync::atomic::{AtomicU8, Ordering},
};

pub const CARGO_LOG_WIDTH: usize = 12;
pub use owo_colors::{style, OwoColorize, Style};
const ARG_GROUP: &str = "output-opts";

#[derive(Debug, Args)]
#[command(
    next_help_heading = "Output Options",
    group = ArgGroup::new(ARG_GROUP).multiple(true),
)]
pub struct OutputOptions {
    /// Whether to emit colors in output.
    #[clap(
            long,
            env = "CARGO_TERM_COLORS",
            default_value_t = ColorMode::Auto,
            global = true,
            group = ARG_GROUP,
        )]
    pub color: ColorMode,

    /// Configures build logging.
    #[clap(
        short,
        long,
        env = "RUST_LOG",
        default_value = "inoculate=info,warn",
        global = true,
        group = ARG_GROUP,
    )]
    pub log: tracing_subscriber::filter::Targets,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, clap::ValueEnum)]
#[repr(u8)]
#[clap(rename_all = "lower")]
pub enum ColorMode {
    /// Determine whether to color output based on whether or not stderr is a
    /// TTY.
    Auto = 0,
    /// Always color output.
    Always = 1,
    /// Never color output.
    Never = 2,
}

// === impl OutputOptions ===
impl OutputOptions {
    pub fn init(&self) -> color_eyre::Result<()> {
        self.color.set_global();
        self.trace_init()
    }
}

// === impl ColorMode ===

static GLOBAL_COLOR_MODE: AtomicU8 = AtomicU8::new(0);

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
