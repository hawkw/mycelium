use crate::color::{Color, SetColor};
use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use embedded_graphics::{
    geometry::Point,
    mono_font::{self, MonoFont, MonoTextStyle},
    pixelcolor::{self, RgbColor},
    text::{self, Text},
    Drawable,
};
use hal_core::framebuffer::{Draw, DrawTarget};
#[derive(Debug)]
pub struct MakeTextWriter<D> {
    mk: fn() -> D,
    settings: TextWriterBuilder,
    next_point: AtomicU64,
    line_len: u32,
    char_height: u32,
    last_line: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct TextWriterBuilder {
    default_color: Color,
    start_point: Point,
}

#[derive(Clone, Debug)]
pub struct TextWriter<'mk, D> {
    target: DrawTarget<D>,
    color: Color,
    mk: &'mk MakeTextWriter<D>,
}

const fn pack_point(Point { x, y }: Point) -> u64 {
    (x as u64) << 32 | y as u64
}

const fn unpack_point(u: u64) -> Point {
    const Y_MASK: u64 = u32::MAX as u64;
    let x = (u >> 32) as i32;
    let y = (u & Y_MASK) as i32;
    Point { x, y }
}

impl<D> fmt::Write for TextWriter<'_, D>
where
    D: Draw,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let curr_packed = self.mk.next_point.load(Ordering::Relaxed);
        let mut curr_point = unpack_point(curr_packed);

        // for a couple of reasons, we don't trust the `embedded-graphics` crate
        // to handle newlines for us:
        //
        // 1. it currently only actually produces a carriage return when a
        //    newline character appears in the *middle* of a string. this means
        //    that strings beginning or ending with newlines (and strings that
        //    are *just* newlines) won't advance the write position the way we'd
        //    expect them to. so, we have to do that part ourself --- it turns
        //    out that most `fmt::Debug`/`fmt::Display` implementations will
        //    write a bunch of strings that begin or end with `\n`.
        // 2. when we reach the bottom of the screen, we want to scroll the
        //    previous text up to make room for a new line of text.
        //    `embedded-graphics` doesn't implement this behavior. because we
        //    want to scroll every time we encounter a newline if we have
        //    reached the bottom of the screen, this means we have to implement
        //    *all* newline handling ourselves.
        //
        // TODO(eliza): currently, our newline handling doesn't honor
        // configurable line height. all lines are simply a single character
        // high. if we want to do something nicer about line height, we'd have
        // to implement that here...
        for mut line in s.split_inclusive('\n') {
            // does this chunk end with a newline? it might not, if:
            // (a) it's the last chunk in a string where newlines only occur in
            //     the beginning/middle.
            // (b) the string being written has no newlines (so
            //     `split_inclusive` will only yield a single chunk)
            let has_newline = line.ends_with('\n');
            if has_newline {
                // if there's a trailing newline, trim it off --- no sense
                // making the `embedded-graphics` crate draw an extra character
                // it will essentially nop for.
                line = &line[..line.len() - 1];
            }

            // if we have reached the bottom of the screen, we'll need to scroll
            // previous framebuffer contents up to make room for new line(s) of
            // text.
            if curr_point.y > self.mk.last_line {
                let ydiff = curr_point.y - self.mk.last_line;
                curr_point = Point {
                    y: self.mk.last_line,
                    x: 10,
                };
                self.target.inner_mut().scroll_vert(ydiff as isize);
            }

            let next_point = if line.is_empty() {
                // if this line is now empty, it was *just* a newline character,
                // so all we have to do is advance the write position.
                curr_point
            } else {
                // otherwise, actually draw the text.
                Text::with_alignment(s, curr_point, self.text_style(), text::Alignment::Left)
                    .draw(&mut self.target)
                    .map_err(|_| fmt::Error)?
            };

            if has_newline {
                // carriage return
                curr_point = Point {
                    y: curr_point.y + self.mk.char_height as i32,
                    x: 10,
                };
            } else {
                curr_point = next_point;
            }
        }

        match self.mk.next_point.compare_exchange(
            curr_packed,
            pack_point(curr_point),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => Ok(()),
            Err(actual_point) => unsafe {
                mycelium_util::unreachable_unchecked!(
                    "lock should guard this, could actually be totally unsync; curr_point={}; actual_point={}",
                    unpack_point(curr_packed),
                    unpack_point(actual_point)
                );
            },
        }
    }
}

impl<'mk, D> SetColor for TextWriter<'mk, D>
where
    D: Draw,
{
    fn set_fg_color(&mut self, color: Color) {
        let color = if color == Color::Default {
            self.mk.settings.default_color
        } else {
            color
        };
        self.color = color;
    }

    fn fg_color(&self) -> Color {
        self.color
    }

    fn set_bold(&mut self, bold: bool) {
        use Color::*;
        let next_color = if bold {
            match self.color {
                Black => BrightBlack,
                Red => BrightRed,
                Green => BrightGreen,
                Yellow => BrightYellow,
                Blue => BrightBlue,
                Magenta => BrightMagenta,
                Cyan => BrightCyan,
                White => BrightWhite,
                x => x,
            }
        } else {
            match self.color {
                BrightBlack => Black,
                BrightRed => Red,
                BrightGreen => Green,
                BrightYellow => Yellow,
                BrightBlue => Blue,
                BrightMagenta => Magenta,
                BrightCyan => Cyan,
                BrightWhite => White,
                x => x,
            }
        };
        self.set_fg_color(next_color);
    }
}

impl<'mk, D> TextWriter<'mk, D>
where
    D: Draw,
{
    fn text_style(&self) -> MonoTextStyle<'static, pixelcolor::Rgb888> {
        use pixelcolor::Rgb888;
        const COLOR_TABLE: [Rgb888; 17] = [
            Rgb888::BLACK,              // black
            Rgb888::new(128, 0, 0),     // red
            Rgb888::new(0, 128, 0),     // green
            Rgb888::new(128, 128, 0),   // yellow
            Rgb888::new(0, 0, 128),     // blue
            Rgb888::new(128, 0, 128),   // magenta
            Rgb888::new(0, 128, 128),   // cyan
            Rgb888::new(192, 192, 192), // white
            Rgb888::new(192, 192, 192), // default
            Rgb888::new(128, 128, 128), // bright black
            Rgb888::new(255, 0, 0),     // bright red
            Rgb888::new(0, 255, 0),     // bright green
            Rgb888::new(255, 255, 0),   // bright yellow
            Rgb888::new(0, 0, 255),     // bright blue
            Rgb888::new(255, 0, 255),   // bright magenta
            Rgb888::new(0, 255, 255),   // bright cyan
            Rgb888::new(255, 255, 255), // bright white
        ];
        MonoTextStyle::new(
            self.mk.settings.get_font(),
            COLOR_TABLE[self.color as usize],
        )
    }
}

// === impl MakeTextWriter ===
impl<D> MakeTextWriter<D> {
    #[must_use]
    pub const fn builder() -> TextWriterBuilder {
        TextWriterBuilder::new()
    }
}

impl<D: Draw> MakeTextWriter<D> {
    #[must_use]
    pub fn new(mk: fn() -> D) -> Self {
        Self::build(mk, TextWriterBuilder::new())
    }

    fn build(mk: fn() -> D, settings: TextWriterBuilder) -> Self {
        let (pixel_width, pixel_height) = {
            let buf = (mk)();
            (buf.width() as u32, buf.height() as u32)
        };
        let text_style = MonoTextStyle::new(settings.get_font(), pixelcolor::Rgb888::WHITE);
        let line_len = Self::line_len(pixel_width, &text_style);
        let char_height = text_style.font.character_size.height;
        let last_line = (pixel_height - char_height - 10) as i32;
        Self {
            settings,
            next_point: AtomicU64::new(pack_point(settings.start_point)),
            char_height,
            mk,
            line_len,
            last_line,
        }
    }

    fn line_len(pixel_width: u32, text_style: &MonoTextStyle<'static, pixelcolor::Rgb888>) -> u32 {
        pixel_width / text_style.font.character_size.width
    }
}

impl<'a, D> crate::writer::MakeWriter<'a> for MakeTextWriter<D>
where
    D: Draw + 'a,
{
    type Writer = TextWriter<'a, D>;

    fn make_writer(&'a self) -> Self::Writer {
        TextWriter {
            color: self.settings.default_color,
            target: (self.mk)().into_draw_target(),
            mk: self,
        }
    }

    fn line_len(&self) -> usize {
        self.line_len as usize
    }
}

impl TextWriterBuilder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            // TODO(eliza): it would be nice if this was configurable via the builder,
            // but it's not, because `MonoFont` is `!Sync` due to containing a trait
            // object without a `Sync` bound...this should be fixed upstream in
            // `embedded-graphics`.
            // font: &mono_font::ascii::FONT_6X13,
            default_color: Color::White,
            start_point: Point { x: 10, y: 10 },
        }
    }

    // #[must_use]
    // pub fn font(self, font: &'static MonoFont<'static>) -> Self {
    //     Self { font, ..self }
    // }

    #[must_use]
    pub fn default_color(self, default_color: Color) -> Self {
        Self {
            default_color,
            ..self
        }
    }

    #[must_use]
    pub fn starting_point(self, start_point: Point) -> Self {
        Self {
            start_point,
            ..self
        }
    }

    #[must_use]
    pub fn build<D: Draw>(self, mk: fn() -> D) -> MakeTextWriter<D> {
        MakeTextWriter::build(mk, self)
    }

    // TODO(eliza): it would be nice if this was configurable via the builder,
    // but it's not, because `MonoFont` is `!Sync` due to containing a trait
    // object without a `Sync` bound...this should be fixed upstream in
    // `embedded-graphics`.
    fn get_font(&self) -> &'static MonoFont<'static> {
        &mono_font::ascii::FONT_6X13
    }
}

impl Default for TextWriterBuilder {
    fn default() -> Self {
        Self::new()
    }
}
