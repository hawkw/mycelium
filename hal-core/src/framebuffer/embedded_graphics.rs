use super::*;
use embedded_graphics_core::{draw_target, geometry, pixelcolor, primitives::Rectangle, Pixel};

#[derive(Copy, Clone, Debug)]
pub struct DrawTarget<D>(D);

impl<D: Draw> DrawTarget<D> {
    pub fn new(draw: D) -> Self {
        Self(draw)
    }

    pub fn size(&self) -> geometry::Size {
        geometry::Size {
            height: self.0.height() as u32,
            width: self.0.width() as u32,
        }
    }

    pub fn inner_mut(&mut self) -> &mut D {
        &mut self.0
    }
}

impl<D: Draw> geometry::Dimensions for DrawTarget<D> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle {
            top_left: geometry::Point { x: 0, y: 0 },
            size: self.size(),
        }
    }
}

impl<D> draw_target::DrawTarget for DrawTarget<D>
where
    D: Draw,
{
    type Error = core::convert::Infallible;
    type Color = pixelcolor::Rgb888;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(geometry::Point { x, y }, color) in pixels {
            // tracing::trace!(x, y, "set_pixel");
            self.0.set_pixel(x as usize, y as usize, color.into());
        }

        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.0.fill(color.into());
        Ok(())
    }
}

impl<C> From<C> for RgbColor
where
    C: pixelcolor::RgbColor,
{
    #[inline]
    fn from(c: C) -> Self {
        Self {
            red: c.r(),
            green: c.g(),
            blue: c.b(),
        }
    }
}
