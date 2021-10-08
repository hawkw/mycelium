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
    pixel_width: usize,
    char_width: usize,
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
        let curr_point = self.mk.next_point.load(Ordering::Relaxed);
        let draw = Text::with_alignment(
            s,
            unpack_point(curr_point),
            default_text_style(),
            text::Alignment::Left,
        )
        .draw(&mut self.target)
        .map_err(|_| fmt::Error);
        let next_point = draw?;
        match self.mk.next_point.compare_exchange(
            curr_point,
            pack_point(next_point),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => Ok(()),
            Err(actual_point) => unsafe {
                mycelium_util::unreachable_unchecked!(
                    "lock should guard this, could actually be totally unsync; curr_point={}; actual_point={}",
                    unpack_point(curr_point),
                    unpack_point(actual_point)
                );
            },
        }
    }
}

impl<D: Draw> MakeTextWriter<D> {
    pub fn new(mk: fn() -> D) -> Self {
        let pixel_width = (mk)().width();
        let text_style = default_text_style();
        let char_width = Self::char_width(pixel_width, &text_style);
        Self {
            next_point: AtomicU64::new(pack_point(Point { x: 10, y: 10 })),
            mk,
            char_width,
            pixel_width,
        }
    }

    fn char_width(
        pixel_width: usize,
        text_style: &MonoTextStyle<'static, pixelcolor::Rgb888>,
    ) -> usize {
        pixel_width / (text_style.font.character_size.width as usize)
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
        self.char_width
    }
}

fn default_text_style() -> MonoTextStyle<'static, pixelcolor::Rgb888> {
    MonoTextStyle::new(&mono_font::ascii::FONT_6X10, pixelcolor::Rgb888::WHITE)
}
