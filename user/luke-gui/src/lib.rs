#![no_std]

extern crate alloc;

use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use libluke::{win_buffer, win_create, win_destroy, win_poll, win_present, Event};

pub use libluke::event_kind;

pub struct Window {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub content_width: u32,
    pub content_height: u32,
    pub buffer: *mut u32,
}

impl Window {
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, i64> {
        let id = win_create(width, height, title)?;
        let buffer = win_buffer(id)?;

        // Window decorations: titlebar = 24, border = 1
        let content_width = width.saturating_sub(2);
        let content_height = height.saturating_sub(26);

        Ok(Self {
            id,
            width,
            height,
            content_width,
            content_height,
            buffer,
        })
    }

    pub fn poll_event(&self) -> Option<Event> {
        win_poll(self.id)
    }

    pub fn present(&self) {
        let _ = win_present(self.id, 0, 0, self.content_width, self.content_height);
    }

    pub fn present_rect(&self, x: u32, y: u32, w: u32, h: u32) {
        let _ = win_present(self.id, x, y, w, h);
    }

    pub fn clear(&mut self, color: u32) {
        let total = (self.content_width * self.content_height) as usize;
        let slice = unsafe { core::slice::from_raw_parts_mut(self.buffer, total) };
        slice.fill(color);
    }

    pub fn put_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.content_width && y < self.content_height {
            let offset = (y * self.content_width + x) as usize;
            unsafe {
                *self.buffer.add(offset) = color;
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = win_destroy(self.id);
    }
}

// Embedded-graphics DrawTarget implementation
impl OriginDimensions for Window {
    fn size(&self) -> Size {
        Size::new(self.content_width, self.content_height)
    }
}

impl DrawTarget for Window {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels.into_iter() {
            if coord.x >= 0
                && (coord.x as u32) < self.content_width
                && coord.y >= 0
                && (coord.y as u32) < self.content_height
            {
                let argb = ((color.r() as u32) << 16)
                    | ((color.g() as u32) << 8)
                    | (color.b() as u32);
                self.put_pixel(coord.x as u32, coord.y as u32, argb);
            }
        }
        Ok(())
    }
}
