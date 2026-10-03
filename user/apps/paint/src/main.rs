#![no_std]
#![no_main]

extern crate alloc;

use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle};
use embedded_graphics::text::Text;
use libluke::{event_kind, exit, sleep};
use luke_gui::Window;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut win = match Window::new("Luke Paint", 360, 240) {
        Ok(w) => w,
        Err(_) => exit(1),
    };

    win.clear(0x00FFFFFF); // White canvas

    let info_style = MonoTextStyle::new(&FONT_6X10, Rgb888::new(100, 100, 100));
    let _ = Text::new("Click & drag to paint with mouse", Point::new(10, 18), info_style).draw(&mut win);
    win.present();

    let mut is_down = false;

    loop {
        let mut dirty = false;
        while let Some(ev) = win.poll_event() {
            match ev.kind {
                event_kind::CLOSE => exit(0),
                event_kind::MOUSE_DOWN => {
                    is_down = true;
                    draw_brush(&mut win, ev.a, ev.b);
                    dirty = true;
                }
                event_kind::MOUSE_UP => {
                    is_down = false;
                }
                event_kind::MOUSE_MOVE => {
                    if is_down {
                        draw_brush(&mut win, ev.a, ev.b);
                        dirty = true;
                    }
                }
                _ => {}
            }
        }

        if dirty {
            win.present();
        }

        sleep(10);
    }
}

fn draw_brush(win: &mut Window, x: i32, y: i32) {
    let style = PrimitiveStyle::with_fill(Rgb888::new(59, 130, 246)); // Vibrant blue
    let circle = Circle::new(Point::new(x - 3, y - 3), 7);
    let _ = circle.into_styled(style).draw(win);
}
