use embedded_graphics_core::{geometry, draw_target::DrawTarget, primitives::Rectangle, pixelcolor::{PixelColor};
use super::*;

// === impl Config ===

impl Config {
    pub fn size(&self) -> geometry::Size {
        geometry::Size {
            height: self.height,
            width: self.width,
        }
    }

    pub fn bounding_box(&self) -> Rectangle {
        Rectangle {
            top_left: geometry::Point { x: 0, y: 0},
            size: self.size(),
        }
    }
}

impl geometry::Dimensions for Framebuffer {
    fn bounding_box(&self) -> Rectangle {
        self.config.bounding_box()
    }
}

// impl<C> DrawTarget for FrameBuffer
// where
//     C: PixelColor,
// {

// }