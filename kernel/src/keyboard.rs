use alloc::collections::VecDeque;
use alloc::string::String;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;

const INPUT_BUFFER_CAPACITY: usize = 256;

/// Global input queue for decoded characters.
static INPUT_BUFFER: Mutex<VecDeque<char>> = Mutex::new(VecDeque::new());

/// Global keyboard state — protected by spinlock since the IRQ handler uses it.
pub static KEYBOARD: Mutex<Keyboard<layouts::De105Key, ScancodeSet1>> = Mutex::new(
    Keyboard::new(
        ScancodeSet1::new(),
        layouts::De105Key,
        HandleControl::MapLettersToUnicode,
    ),
);

/// Process a raw scancode from the i8042 controller (PS/2 port 0x60).
/// Returns `Some(char)` if the scancode results in a decoded character.
pub fn process_scancode(scancode: u8) -> Option<char> {
    let mut kb = KEYBOARD.lock();
    if let Ok(Some(key_event)) = kb.add_byte(scancode) {
        let code = key_event.code;
        let pressed = key_event.state == pc_keyboard::KeyState::Down;
        let decoded = kb.process_keyevent(key_event);
        let ch = match decoded {
            Some(DecodedKey::Unicode(c)) => Some(c),
            _ => None,
        };

        crate::input::push_event(crate::input::InputEvent::Key {
            ch,
            code,
            pressed,
        });

        return ch;
    }
    None
}

/// Push a decoded character into the input ring buffer.
/// If the buffer reaches capacity, the oldest unread character is dropped.
pub fn push_char(ch: char) {
    let mut queue = INPUT_BUFFER.lock();
    if queue.len() >= INPUT_BUFFER_CAPACITY {
        let _ = queue.pop_front();
    }
    queue.push_back(ch);
}

/// Try to pop a character from the input ring buffer without blocking.
pub fn pop_char() -> Option<char> {
    INPUT_BUFFER.lock().pop_front()
}

/// Block the calling thread until a character is available.
/// Yields execution to other threads between polling iterations.
pub fn read_char() -> char {
    loop {
        if let Some(ch) = pop_char() {
            return ch;
        }
        crate::scheduler::yield_now();
    }
}

/// Read a line of text terminated by '\n' or '\r'.
/// Handles backspace ('\x08'), echoes characters to stdout, and returns the entered string.
pub fn read_line() -> String {
    let mut line = String::new();
    loop {
        let ch = read_char();
        match ch {
            '\r' | '\n' => {
                crate::print!("\n");
                break;
            }
            '\x08' => {
                if !line.is_empty() {
                    line.pop();
                    crate::print!("\x08");
                }
            }
            c => {
                line.push(c);
                crate::print!("{}", c);
            }
        }
    }
    line
}
