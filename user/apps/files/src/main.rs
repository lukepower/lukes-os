#![no_std]
#![no_main]

extern crate alloc;

use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_8X13_BOLD};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use libluke::{event_kind, exit, sleep};
use luke_gui::Window;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut win = match Window::new("File Browser", 340, 220) {
        Ok(w) => w,
        Err(_) => exit(1),
    };

    win.clear(0x000F172A); // Very dark slate

    // Header bar
    let header_rect = Rectangle::new(Point::new(0, 0), Size::new(340, 24));
    let _ = header_rect
        .into_styled(PrimitiveStyle::with_fill(Rgb888::new(30, 41, 59)))
        .draw(&mut win);

    let header_style = MonoTextStyle::new(&FONT_8X13_BOLD, Rgb888::new(241, 245, 249));
    let _ = Text::new("Location: /bin", Point::new(8, 16), header_style).draw(&mut win);

    // File list
    let item_style = MonoTextStyle::new(&FONT_6X10, Rgb888::new(226, 232, 240));
    let items = [
        " [FILE]  clock        (GUI Uptime Clock)",
        " [FILE]  paint        (GUI Canvas Drawing)",
        " [FILE]  files        (GUI File Browser)",
        " [FILE]  hello        (Ring 3 Hello World)",
    ];

    let mut y = 45;
    for item in items.iter() {
        let _ = Text::new(item, Point::new(12, y), item_style).draw(&mut win);
        y += 24;
    }

    win.present();

    loop {
        while let Some(ev) = win.poll_event() {
            if ev.kind == event_kind::CLOSE {
                exit(0);
            }
        }
        sleep(50);
    }
}
