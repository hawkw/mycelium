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
#[derive(Debug)]
pub struct MakeTextWriter<D> {
    mk: fn() -> D,
    next_point: AtomicU64,
    line_len: u32,
    char_height: u32,
    last_line: i32,
}

#[derive(Clone, Debug)]
pub struct TextWriter<'mk, D> {
    target: DrawTarget<D>,
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

impl<'mk, D> fmt::Write for TextWriter<'mk, D>
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
                Text::with_alignment(s, curr_point, default_text_style(), text::Alignment::Left)
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

impl<D: Draw> MakeTextWriter<D> {
    pub fn new(mk: fn() -> D) -> Self {
        Self::new_at(mk, Point { x: 10, y: 10 })
    }

    pub fn new_at(mk: fn() -> D, point: Point) -> Self {
        let (pixel_width, pixel_height) = {
            let buf = (mk)();
            (buf.width() as u32, buf.height() as u32)
        };
        let text_style = default_text_style();
        let line_len = Self::line_len(pixel_width, &text_style);
        let char_height = text_style.font.character_size.height;
        let last_line = (pixel_height - char_height - 10) as i32;
        Self {
            next_point: AtomicU64::new(pack_point(point)),
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
            target: (self.mk)().into_draw_target(),
            mk: self,
        }
    }

    fn line_len(&self) -> usize {
        self.line_len as usize
    }
}

fn default_text_style() -> MonoTextStyle<'static, pixelcolor::Rgb888> {
    MonoTextStyle::new(&mono_font::ascii::FONT_6X13, pixelcolor::Rgb888::WHITE)
}
