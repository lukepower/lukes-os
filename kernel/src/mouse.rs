use x86_64::instructions::port::Port;
use crate::input::{push_event, InputEvent};
use spin::Mutex;

const PS2_DATA_PORT: u16 = 0x60;
const PS2_STATUS_PORT: u16 = 0x64;
const PS2_COMMAND_PORT: u16 = 0x64;

struct MouseState {
    phase: u8,
    bytes: [u8; 3],
}

static MOUSE_STATE: Mutex<MouseState> = Mutex::new(MouseState {
    phase: 0,
    bytes: [0; 3],
});

fn mouse_wait_write() {
    let mut status_port = Port::<u8>::new(PS2_STATUS_PORT);
    for _ in 0..100_000 {
        if unsafe { status_port.read() } & 0x02 == 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

fn mouse_wait_read() {
    let mut status_port = Port::<u8>::new(PS2_STATUS_PORT);
    for _ in 0..100_000 {
        if unsafe { status_port.read() } & 0x01 == 1 {
            return;
        }
        core::hint::spin_loop();
    }
}

fn mouse_write(byte: u8) {
    mouse_wait_write();
    let mut cmd_port = Port::<u8>::new(PS2_COMMAND_PORT);
    unsafe { cmd_port.write(0xD4) }; // Tell controller to route byte to mouse
    mouse_wait_write();
    let mut data_port = Port::<u8>::new(PS2_DATA_PORT);
    unsafe { data_port.write(byte) };
}

fn mouse_read() -> u8 {
    mouse_wait_read();
    let mut data_port = Port::<u8>::new(PS2_DATA_PORT);
    unsafe { data_port.read() }
}

pub fn init() {
    // 1. Enable auxiliary mouse port
    mouse_wait_write();
    let mut cmd_port = Port::<u8>::new(PS2_COMMAND_PORT);
    unsafe { cmd_port.write(0xA8) };

    // 2. Enable IRQ12 in controller configuration byte
    mouse_wait_write();
    unsafe { cmd_port.write(0x20) }; // Read config command
    let mut status = mouse_read();
    status |= 0x02; // Enable IRQ12 (bit 1)
    status &= !0x20; // Clear disable mouse clock bit (bit 5)

    mouse_wait_write();
    unsafe { cmd_port.write(0x60) }; // Write config command
    mouse_wait_write();
    let mut data_port = Port::<u8>::new(PS2_DATA_PORT);
    unsafe { data_port.write(status) };

    // 3. Set default settings (0xF6)
    mouse_write(0xF6);
    let _ = mouse_read(); // ACK (0xFA)

    // 4. Enable packet streaming (0xF4)
    mouse_write(0xF4);
    let _ = mouse_read(); // ACK (0xFA)

    crate::serial_println!("[OK] PS/2 mouse initialized");
}

pub fn handle_interrupt() {
    let mut port = Port::<u8>::new(PS2_DATA_PORT);
    let byte: u8 = unsafe { port.read() };

    let mut state = MOUSE_STATE.lock();
    match state.phase {
        0 => {
            // First byte must have bit 3 set (always 1)
            if byte & 0x08 != 0 {
                state.bytes[0] = byte;
                state.phase = 1;
            }
        }
        1 => {
            state.bytes[1] = byte;
            state.phase = 2;
        }
        2 => {
            state.bytes[2] = byte;
            state.phase = 0;

            let flags = state.bytes[0];
            let raw_x = state.bytes[1];
            let raw_y = state.bytes[2];

            // Parse sign bits
            let x_sign = (flags & 0x10) != 0;
            let y_sign = (flags & 0x20) != 0;
            let x_overflow = (flags & 0x40) != 0;
            let y_overflow = (flags & 0x80) != 0;

            if !x_overflow && !y_overflow {
                let dx = if x_sign {
                    (raw_x as i16) | !0xFF
                } else {
                    raw_x as i16
                };

                let dy = if y_sign {
                    (raw_y as i16) | !0xFF
                } else {
                    raw_y as i16
                };

                // In PS/2, positive dy means upward motion, in screen coords positive is downward
                let dy_screen = -dy;

                if dx != 0 || dy_screen != 0 {
                    push_event(InputEvent::MouseMove { dx, dy: dy_screen });
                }

                // Buttons
                let left = (flags & 0x01) != 0;
                let right = (flags & 0x02) != 0;
                let middle = (flags & 0x04) != 0;

                // Push button events if needed
                static PREV_BUTTONS: Mutex<(bool, bool, bool)> = Mutex::new((false, false, false));
                let mut prev = PREV_BUTTONS.lock();
                if left != prev.0 {
                    push_event(InputEvent::MouseButton { button: 0, pressed: left });
                    prev.0 = left;
                }
                if right != prev.1 {
                    push_event(InputEvent::MouseButton { button: 1, pressed: right });
                    prev.1 = right;
                }
                if middle != prev.2 {
                    push_event(InputEvent::MouseButton { button: 2, pressed: middle });
                    prev.2 = middle;
                }
            }
        }
        _ => {
            state.phase = 0;
        }
    }
}
