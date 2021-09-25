use core::ops::{Deref, DerefMut};

pub trait Draw {
    /// Return the width of the framebuffer in pixels.
    fn width(&self) -> usize;

    /// Returns the height of the framebuffer in pixels.
    fn height(&self) -> usize;

    /// Set the pixel at position (`x`, `y`) to the provided `color`.
    fn set_pixel(&mut self, x: usize, y: usize, color: RgbColor) -> &mut Self;

    /// Draw a horizontal line of length `len` at height `y`.
    ///
    /// By default, this method calls `set_pixel` in a loop. This _works_, but
    /// implementations can almost certainly provide a more optimal
    /// implementation, and are thus encouraged to override this method.
    fn line_horiz(&mut self, y: usize, len: usize, color: RgbColor) -> &mut Self {
        for x in 0..len {
            self.set_pixel(x, y, color);
        }
        self
    }

    /// Draw a vertical line of length `len` at column `y`.
    ///
    /// By default, this method calls `set_pixel` in a loop. This _works_, but
    /// implementations can almost certainly provide a more optimal
    /// implementation, and are thus encouraged to override this method.
    fn line_vert(&mut self, x: usize, len: usize, color: RgbColor) -> &mut Self {
        for y in 0..len {
            self.set_pixel(x, y, color);
        }
        self
    }

    #[inline]
    fn fill_row(&mut self, y: usize, color: RgbColor) -> &mut Self {
        self.line_horiz(y, self.width(), color)
    }

    #[inline]
    fn fill_col(&mut self, x: usize, color: RgbColor) -> &mut Self {
        self.line_vert(x, self.height(), color)
    }

    /// Fill the entire framebuffer with the provided color.
    ///
    /// By default, this method calls `set_pixel` in a loop. This _works_, but
    /// implementations can almost certainly provide a more optimal
    /// implementation, and are thus encouraged to override this method.
    fn fill(&mut self, color: RgbColor) -> &mut Self {
        for y in 0..self.height() {
            self.line_horiz(y, self.width(), color);
        }
        self
    }

    /// Clear the entire framebuffer.
    ///
    /// By default, this method calls `set_pixel` in a loop. This _works_, but
    /// implementations can almost certainly provide a more optimal
    /// implementation, and are thus encouraged to override this method.
    fn clear(&mut self) -> &mut Self {
        self.fill(RgbColor::BLACK);
        self
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl RgbColor {
    pub const BLACK: Self = Self { red: 0, green: 0, blue: 0};
    pub const RED: Self = Self { red: 255, green: 0, blue: 0};
    pub const GREEN: Self = Self { red: 0, green: 255, blue: 0};
    pub const BLUE: Self = Self { red: 0, green: 0, blue: 255};
}

impl<'lock, D> Draw for mycelium_util::sync::spin::MutexGuard<'lock, D>
where
    D: Draw,
{
    #[inline]
    fn width(&self) -> usize {
        self.deref().width()
    }

    #[inline]
    fn height(&self) -> usize {
        self.deref().height()
    }

    #[inline]
    fn set_pixel(&mut self, x: usize, y: usize, color: RgbColor) -> &mut Self {
        self.deref_mut().set_pixel(x, y, color);
        self
    }

    #[inline]
    fn line_horiz(&mut self, y: usize, len: usize, color: RgbColor) -> &mut Self {
        self.deref_mut().line_horiz(y, len, color);
        self
    }

    #[inline]
    fn line_vert(&mut self, x: usize, len: usize, color: RgbColor) -> &mut Self {
        self.deref_mut().line_vert(x, len, color);
        self
    }

    #[inline]
    fn fill_row(&mut self, y: usize, color: RgbColor) -> &mut Self {
        self.deref_mut().fill_row(y, color);
        self
    }

    #[inline]
    fn fill_col(&mut self, x: usize, color: RgbColor) -> &mut Self {
        self.deref_mut().fill_col(x, color);
        self
    }

    #[inline]
    fn fill(&mut self, color: RgbColor) -> &mut Self {
        self.deref_mut().fill(color);
        self
    }

    #[inline]
    fn clear(&mut self) -> &mut Self {
        self.deref_mut().clear();
        self
    }
}