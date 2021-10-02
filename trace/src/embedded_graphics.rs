use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use embedded_graphics::{
    geometry::Point,
    pixelcolor,
    text::{mono_font, MonoTextStyle},
};
use hal_core::framebuffer::{Draw, DrawTarget};
use mycelium_util::sync::spin;

#[derive(Clone, Debug)]
pub struct MakeTextWriter<D> {
    lock: &'static spin::Mutex<D>,
    text_style: MonoTextStyle,
    next_point: AtomicU64,
    pixel_width: usize,
    char_width: usize,
}

#[derive(Clone, Debug)]
pub struct TextWriter<'mk, D> {
    target: DrawTarget<spin::MutexGuard<'static, D>>,
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
        let next_point = Text::new(s, unpack_point(curr_point), &self.mk.text_style)
            .draw(&mut self.target)
            .map_err(|_| fmt::Error)?;
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

impl<D> MakeTextWriter<D> {
    pub fn default_text_style() -> MonoTextStyle {
        MonoTextStyle::new(&mono_font::ascii::FONT_6X10, pixelcolor::Rgb888::WHITE)
    }
}

impl<D: Draw> MakeTextWriter {
    pub fn new(framebuffer: &'static spin::Mutex<D>) -> Self {
        let pixel_width = framebuffer.lock().width();
        let text_style = Self::default_text_style();
        let char_width = char_width(pixel_width, &text_style);
        Self {
            next_point: AtomicU64::new(pack_point(Point { x: 0, y: 0 })),
            lock: framebuffer,
            char_width,
            pixel_width,
        }
    }

    pub fn with_text_style(self, text_style: MonoTextStyle) -> Self {
        let char_width = char_width(self.pixel_width, &text_style);
        Self {
            text_style,
            char_width,
            ..self
        }
    }

    fn char_width(pixel_width: usize, text_style: &MonoTextStyle) -> usize {
        pixel_width / text_style.font.size.width
    }
}

impl<'a, D: Draw> MakeWriter<'a> for MakeTextWriter<D> {
    type Writer = TextWriter<'a, D>;

    fn make_writer(&'a self) -> Self::Writer {
        TextWriter {
            target: self.lock.lock().into_draw_target(),
            mk: self,
        }
    }

    fn line_len(&self) -> usize {
        self.char_width
    }
}
