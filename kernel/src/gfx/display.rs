use bootloader_api::info::{FrameBuffer, PixelFormat};
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayPixelFormat {
    Rgb,
    Bgr,
    Unknown,
}

pub struct Display {
    pub front: *mut u8,
    pub pitch: usize,
    pub width: usize,
    pub height: usize,
    pub bpp: usize,
    pub format: DisplayPixelFormat,
}

unsafe impl Send for Display {}
unsafe impl Sync for Display {}

pub static DISPLAY: Mutex<Option<Display>> = Mutex::new(None);

pub fn init(framebuffer: &'static mut FrameBuffer) {
    let info = framebuffer.info();
    let buf = framebuffer.buffer_mut();

    let format = match info.pixel_format {
        PixelFormat::Rgb => DisplayPixelFormat::Rgb,
        PixelFormat::Bgr => DisplayPixelFormat::Bgr,
        PixelFormat::U8 => {
            crate::serial_println!("[WARN] PixelFormat::U8 is not fully supported, assuming grayscale/unknown");
            DisplayPixelFormat::Unknown
        }
        _ => {
            crate::serial_println!("[WARN] Unknown framebuffer PixelFormat, assuming BGR");
            DisplayPixelFormat::Unknown
        }
    };

    let display = Display {
        front: buf.as_mut_ptr(),
        pitch: info.stride * info.bytes_per_pixel,
        width: info.width,
        height: info.height,
        bpp: info.bytes_per_pixel,
        format,
    };

    *DISPLAY.lock() = Some(display);
}
