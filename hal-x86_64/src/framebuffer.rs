use core::{
    fmt,
    ops::{Deref, DerefMut},
};
use hal_core::framebuffer::{Draw, RgbColor};
use volatile::Volatile;

pub struct Framebuffer<'buf, B: Deref<Target = [u8]>> {
    /// A reference to the framebuffer itself.
    ///
    /// This is wrapped in a [`Volatile`] cell because we will typically never
    /// read from the framebuffer, so we need to use volatile writes every time
    /// it's written to.
    buf: Volatile<B>,

    /// Length of the actual framebuffer array.
    ///
    /// This is stored in the struct because when we construct a new
    /// `Framebuffer`, we wrap `buf` in a `Volatile` cell, and calling random
    /// methods like `[u8]::len()` on it becomes kind of a pain.
    len: usize,

    /// Framebuffer configuration values.
    cfg: &'buf Config,
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

impl<'buf, B> Framebuffer<'buf, B>
where
    B: Deref<Target = [u8]> + DerefMut,
{
    pub fn new(cfg: &'buf Config, buf: B) -> Self {
        let len = buf[..].len();
        Self {
            cfg,
            buf: Volatile::new(buf),
            len,
        }
    }

    pub fn clear(&mut self) -> &mut Self {
        self.buf.fill(0);
        self
    }

    pub fn set_pixel_rgb(&mut self, x: usize, y: usize, color: RgbColor) -> &mut Self {
        let px_bytes = self.cfg.px_bytes;
        let start = (y * self.cfg.line_len + x) * px_bytes;
        let end = start + px_bytes;

        let px_vals = &self.cfg.px_kind.convert_rgb(color)[..px_bytes];
        self.buf.index_mut(start..end).copy_from_slice(px_vals);
        self
    }
}

impl<'buf, B> Draw for Framebuffer<'buf, B>
where
    B: Deref<Target = [u8]> + DerefMut,
{
    fn height(&self) -> usize {
        self.cfg.height
    }

    fn width(&self) -> usize {
        self.cfg.width
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: RgbColor) -> &mut Self {
        self.set_pixel_rgb(x, y, color)
    }

    fn scroll_vert(&mut self, amount: isize) -> &mut Self {
        if amount < 0 {
            todo!("eliza: handle negatives!")
        }
        let amount_px = (amount as usize * self.cfg.line_len) * self.cfg.px_bytes;
        self.buf.copy_within(amount_px.., 0);
        let revealed_start = self.len - amount_px;
        self.buf.index_mut(revealed_start..).fill(0);
        self
    }
}

// `Volatile<B>` is only `Debug` if `<B as Deref>::Target: Copy`,
// so we must implement this manually.
impl<'buf, B> fmt::Debug for Framebuffer<'buf, B>
where
    B: Deref<Target = [u8]>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { cfg, len, buf: _ } = self;
        f.debug_struct("Framebuffer")
            .field("len", len)
            .field("cfg", cfg)
            // don't print every pixel value in the entire framebuffer...
            .field("buf", &format_args!("[..]"))
            .finish()
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
