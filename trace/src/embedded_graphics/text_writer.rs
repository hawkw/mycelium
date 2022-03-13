use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use embedded_graphics::{
    geometry::Point,
    mono_font::{self, MonoTextStyle},
    pixelcolor::{self, RgbColor},
    text::{self, Text},
    Drawable,
};
use hal_core::framebuffer::{Draw, DrawTarget};
#[derive(Clone, Debug)]
pub struct TextWriter<D> {
    target: DrawTarget<D>,
    line_len: u32,
    char_height: u32,
    last_line: i32,
    indent: u32,
    style: MonoTextStyle<'static, pixelcolor::Rgb888>,
    point: Point,
}

impl<D: Draw> TextWriter<D> {
    pub fn new(at: Point, style: MonoTextStyle<'static, pixelcolor::Rgb888>, target: D) -> Self {
        let pixel_width = target.width() as u32;
        let pixel_height = target.height() as u32;
        let line_len = Self::line_len(pixel_width, &style);
        let indent = point.x;
        let last_line = (pixel_height - char_height - point.y) as i32;
        Self {
            point,
            style,
            char_height,
            indent,
            target: DrawTarget::new(target),
        }
    }

    fn line_len(pixel_width: u32, text_style: &MonoTextStyle<'static, pixelcolor::Rgb888>) -> u32 {
        pixel_width / text_style.font.character_size.width
    }
}

impl<D> fmt::Write for TextWriter<D>
where
    D: Draw,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
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
            if self.point.y > self.last_line {
                let ydiff = self.point.y - self.last_line;
                self.point = Point {
                    y: self.last_line,
                    x: self.indent,
                };
                self.target.inner_mut().scroll_vert(ydiff as isize);
            }

            let next_point = if line.is_empty() {
                // if this line is now empty, it was *just* a newline character,
                // so all we have to do is advance the write position.
                curr_point
            } else {
                // otherwise, actually draw the text.
                Text::with_alignment(s, curr_point, &self.style, text::Alignment::Left)
                    .draw(&mut self.target)
                    .map_err(|_| fmt::Error)?
            };

            if has_newline {
                // carriage return
                self.point = Point {
                    y: curr_point.y + self.char_height as i32,
                    x: self.indent,
                };
            } else {
                self.point = next_point;
            }
        }

        Ok(())
    }
}
