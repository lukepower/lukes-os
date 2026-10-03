#![no_std]
#![no_main]

extern crate alloc;

use embedded_graphics::mono_font::ascii::FONT_9X18_BOLD;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use libluke::{event_kind, exit, sleep, time};
use luke_gui::Window;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut win = match Window::new("Uptime Clock", 220, 100) {
        Ok(w) => w,
        Err(_) => exit(1),
    };

    let text_style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb888::new(255, 255, 255));
    let mut last_sec = u64::MAX;

    loop {
        // Poll for window events
        while let Some(ev) = win.poll_event() {
            if ev.kind == event_kind::CLOSE {
                exit(0);
            }
        }

        let ms = time();
        let sec = ms / 1000;
        if sec != last_sec {
            last_sec = sec;
            let mins = sec / 60;
            let s_rem = sec % 60;

            let mut time_str = [b'0'; 8]; // "00:00:00"
            let hours = mins / 60;
            let m_rem = mins % 60;

            time_str[0] = b'0' + ((hours / 10) % 10) as u8;
            time_str[1] = b'0' + (hours % 10) as u8;
            time_str[2] = b':';
            time_str[3] = b'0' + ((m_rem / 10) % 10) as u8;
            time_str[4] = b'0' + (m_rem % 10) as u8;
            time_str[5] = b':';
            time_str[6] = b'0' + ((s_rem / 10) % 10) as u8;
            time_str[7] = b'0' + (s_rem % 10) as u8;

            win.clear(0x001E293B); // Slate dark blue

            if let Ok(clock_s) = core::str::from_utf8(&time_str) {
                let text = Text::new(clock_s, Point::new(60, 45), text_style);
                let _ = text.draw(&mut win);
            }

            win.present();
        }

        sleep(50);
    }
}
