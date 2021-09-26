use core::slice;
use hal_core::framebuffer::{Draw, RgbColor};

#[derive(Debug)]
pub struct Framebuffer<'buf> {
    buf: &'buf mut [u8],
    cfg: Config,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// The framebuffer height in pixels
    pub height: usize,
    /// The framebuffer width in pixels.
    pub width: usize,
    pub px_bytes: usize,
    pub line_len: usize,
    pub px_kind: PixelKind,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PixelKind {
    Bgr,
    Rgb,
    Gray,
}

#[cfg(feature = "embedded-graphics-core")]
#[doc(cfg(feature = "embedded-graphics-core"))]
mod embedded_graphics;
#[cfg(feature = "embedded-graphics-core")]
#[doc(cfg(feature = "embedded-graphics-core"))]
pub use self::embedded_graphics::*;

impl<'buf> Framebuffer<'buf> {
    pub fn new(cfg: Config, buf: &'buf mut [u8]) -> Self {
        Self { cfg, buf }
    }

    pub fn clear(&mut self) -> &mut Self {
        self.buf.fill(0);
        self
    }

    pub fn set_pixel_rgb(&mut self, x: usize, y: usize, color: RgbColor) -> &mut Self {
        let px_bytes = self.cfg.px_bytes;
        let pos = (y * self.cfg.line_len + x) * px_bytes;

        let slice = &mut self.buf[pos..(pos + px_bytes)];
        let px_vals = &self.cfg.px_kind.convert_rgb(color)[..px_bytes];
        for (byte, &val) in slice.iter_mut().zip(px_vals) {
            let byte = byte as *mut u8;
            unsafe {
                // Safety: this is only unsafe because we perform a volatile
                // write here. if we performed a normal `*byte = val` write,
                // this would be perfectly safe, as we have mutable ownership
                // over the buffer. however, we need a volatile write, since
                // the contents of the framebuffer may not be read, and we can't
                // let rustc optimize this out. `ptr::write_volatile` is unsafe,
                // so this is unsafe.
                ptr::write_volatile(byte, val);
            }
        }
        self
    }
}

impl<'buf> Draw for Framebuffer<'buf> {
    fn height(&self) -> usize {
        self.cfg.height
    }

    fn width(&self) -> usize {
        self.cfg.width
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: RgbColor) -> &mut Self {
        self.set_pixel_rgb(x, y, color)
    }
}

impl PixelKind {
    fn convert_rgb(self, RgbColor { red, green, blue }: RgbColor) -> [u8; 4] {
        match self {
            PixelKind::Bgr => [blue, green, red, 0],
            PixelKind::Rgb => [red, green, blue, 0],
            PixelKind::Gray => [Self::rgb_to_luminance(red, green, blue), 0, 0, 0],
        }
    }

    fn rgb_to_luminance(r: u8, g: u8, b: u8) -> u8 {
        // Thanks to @mystor for magic numbers!
        ((21 * (r as u32) + 72 * (g as u32) + 7 * (b as u32)) / 100) as u8
    }
}
