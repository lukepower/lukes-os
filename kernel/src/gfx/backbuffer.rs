use crate::gfx::display::{DisplayPixelFormat, DISPLAY};
use crate::gfx::rect::{DirtyRegion, Rect};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Size};
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::pixelcolor::RgbColor;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::Pixel;
use spin::Mutex;
use x86_64::structures::paging::{
    Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

pub const BACKBUFFER_VIRT_ADDR: u64 = 0x4444_8000_0000;

pub struct BackBuffer {
    buffer: *mut u32,
    width: usize,
    height: usize,
    pitch_pixels: usize,
    dirty: DirtyRegion,
}

unsafe impl Send for BackBuffer {}
unsafe impl Sync for BackBuffer {}

pub static BACKBUFFER: Mutex<Option<BackBuffer>> = Mutex::new(None);

pub fn init(
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut crate::memory::BootInfoFrameAllocator,
) {
    let display_guard = DISPLAY.lock();
    let display = match display_guard.as_ref() {
        Some(d) => d,
        None => {
            crate::serial_println!("[WARN] Cannot init BackBuffer: Display not initialized");
            return;
        }
    };

    let width = display.width;
    let height = display.height;
    let total_bytes = width * height * 4;
    let pages_needed = (total_bytes + 4095) / 4096;

    let start_frame = frame_allocator
        .allocate_contiguous(pages_needed)
        .expect("Failed to allocate contiguous physical frames for BackBuffer");

    let start_page: Page<Size4KiB> =
        Page::containing_address(VirtAddr::new(BACKBUFFER_VIRT_ADDR));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for i in 0..pages_needed {
        let page = start_page + i as u64;
        let frame = start_frame + i as u64;
        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .expect("Failed to map BackBuffer virtual page")
                .flush();
        }
    }

    let buffer = BACKBUFFER_VIRT_ADDR as *mut u32;
    unsafe {
        core::ptr::write_bytes(buffer as *mut u8, 0, total_bytes);
    }

    let mut bb = BackBuffer {
        buffer,
        width,
        height,
        pitch_pixels: width,
        dirty: DirtyRegion::empty(),
    };
    bb.mark_dirty(Rect::new(0, 0, width as u32, height as u32));

    crate::serial_println!(
        "[OK] BackBuffer initialized: {}x{} ({} KiB mapped at 0x{:X})",
        width,
        height,
        total_bytes / 1024,
        BACKBUFFER_VIRT_ADDR
    );

    *BACKBUFFER.lock() = Some(bb);
}

impl BackBuffer {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn buffer_mut(&mut self) -> *mut u32 {
        self.buffer
    }

    pub fn buffer_slice_mut(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.buffer, self.width * self.height) }
    }

    pub fn mark_dirty(&mut self, rect: Rect) {
        self.dirty.mark_dirty(rect);
    }

    pub fn clear(&mut self, color: u32) {
        let total = self.width * self.height;
        let slice = unsafe { core::slice::from_raw_parts_mut(self.buffer, total) };
        slice.fill(color);
        self.mark_dirty(Rect::new(0, 0, self.width as u32, self.height as u32));
    }

    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            unsafe {
                *self.buffer.add(y * self.pitch_pixels + x) = color;
            }
            self.dirty.mark_dirty(Rect::new(x as i32, y as i32, 1, 1));
        }
    }

    pub fn fill_rect(&mut self, rect: Rect, color: u32) {
        let bounds = Rect::new(0, 0, self.width as u32, self.height as u32);
        let clamped = match rect.clamp_to(&bounds) {
            Some(r) => r,
            None => return,
        };

        for y in clamped.y..(clamped.y + clamped.height as i32) {
            let row_start = y as usize * self.pitch_pixels + clamped.x as usize;
            unsafe {
                let ptr = self.buffer.add(row_start);
                for dx in 0..clamped.width as usize {
                    *ptr.add(dx) = color;
                }
            }
        }
        self.dirty.mark_dirty(clamped);
    }

    pub fn copy_rect(
        &mut self,
        src_buf: &[u32],
        src_stride: usize,
        src_rect: Rect,
        dst_x: i32,
        dst_y: i32,
    ) {
        let bounds = Rect::new(0, 0, self.width as u32, self.height as u32);
        for sy in 0..src_rect.height as i32 {
            let dy = dst_y + sy;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            for sx in 0..src_rect.width as i32 {
                let dx = dst_x + sx;
                if dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                let src_idx = (src_rect.y + sy) as usize * src_stride + (src_rect.x + sx) as usize;
                if src_idx < src_buf.len() {
                    let pixel = src_buf[src_idx];
                    unsafe {
                        *self.buffer.add(dy as usize * self.pitch_pixels + dx as usize) = pixel;
                    }
                }
            }
        }
        let affected = Rect::new(dst_x, dst_y, src_rect.width, src_rect.height);
        if let Some(clamped) = affected.clamp_to(&bounds) {
            self.dirty.mark_dirty(clamped);
        }
    }

    /// Present dirty regions to the front buffer (screen)
    pub fn present(&mut self) {
        let dirty_rect = match self.dirty.take() {
            Some(r) => r,
            None => return,
        };

        let screen_bounds = Rect::new(0, 0, self.width as u32, self.height as u32);
        let clamped = match dirty_rect.clamp_to(&screen_bounds) {
            Some(r) => r,
            None => return,
        };

        let display_guard = DISPLAY.lock();
        let display = match display_guard.as_ref() {
            Some(d) => d,
            None => return,
        };

        let is_bgr = display.format != DisplayPixelFormat::Rgb; // Bootloader default is BGR

        for y in clamped.y..(clamped.y + clamped.height as i32) {
            let back_row_offset = y as usize * self.pitch_pixels + clamped.x as usize;
            let front_row_offset = y as usize * display.pitch + clamped.x as usize * display.bpp;

            unsafe {
                let back_ptr = self.buffer.add(back_row_offset);
                let front_ptr = display.front.add(front_row_offset);

                if display.bpp == 4 {
                    if is_bgr {
                        // In BackBuffer, u32 is stored as 0x00RRGGBB (or BGR in memory).
                        // Let's copy row-wise or convert if needed:
                        // If BackBuffer stores 0x00RRGGBB (native little endian: byte0=B, byte1=G, byte2=R, byte3=0),
                        // and display is BGR (byte0=B, byte1=G, byte2=R, byte3=unused),
                        // then copy_nonoverlapping is directly identical!
                        core::ptr::copy_nonoverlapping(
                            back_ptr as *const u8,
                            front_ptr,
                            clamped.width as usize * 4,
                        );
                    } else {
                        // RGB: swap B and R
                        for x in 0..clamped.width as usize {
                            let val = *back_ptr.add(x);
                            let b = (val & 0xFF) as u8;
                            let g = ((val >> 8) & 0xFF) as u8;
                            let r = ((val >> 16) & 0xFF) as u8;
                            let target = front_ptr.add(x * 4);
                            *target = r;
                            *target.add(1) = g;
                            *target.add(2) = b;
                            *target.add(3) = 0;
                        }
                    }
                } else if display.bpp == 3 {
                    for x in 0..clamped.width as usize {
                        let val = *back_ptr.add(x);
                        let b = (val & 0xFF) as u8;
                        let g = ((val >> 8) & 0xFF) as u8;
                        let r = ((val >> 16) & 0xFF) as u8;
                        let target = front_ptr.add(x * 3);
                        if is_bgr {
                            *target = b;
                            *target.add(1) = g;
                            *target.add(2) = r;
                        } else {
                            *target = r;
                            *target.add(1) = g;
                            *target.add(2) = b;
                        }
                    }
                }
            }
        }
    }
}

impl OriginDimensions for BackBuffer {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}

impl DrawTarget for BackBuffer {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), core::convert::Infallible>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels.into_iter() {
            if coord.x >= 0 && coord.y >= 0 {
                let x = coord.x as usize;
                let y = coord.y as usize;
                let u32_color = ((color.r() as u32) << 16)
                    | ((color.g() as u32) << 8)
                    | (color.b() as u32);
                self.put_pixel(x, y, u32_color);
            }
        }
        Ok(())
    }

    fn fill_solid(
        &mut self,
        area: &Rectangle,
        color: Self::Color,
    ) -> Result<(), core::convert::Infallible> {
        let u32_color = ((color.r() as u32) << 16)
            | ((color.g() as u32) << 8)
            | (color.b() as u32);
        let rect = Rect::new(
            area.top_left.x,
            area.top_left.y,
            area.size.width,
            area.size.height,
        );
        self.fill_rect(rect, u32_color);
        Ok(())
    }
}
