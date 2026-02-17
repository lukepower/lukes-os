// Keyboard driver using the pc-keyboard crate.
// Supports multiple layouts (default: German De105Key).
// Handles Shift, CapsLock, AltGr, and extended scancodes.

use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;

/// Global keyboard state — protected by spinlock since the IRQ handler uses it.
pub static KEYBOARD: Mutex<Keyboard<layouts::De105Key, ScancodeSet1>> = Mutex::new(
    Keyboard::new(
        ScancodeSet1::new(),
        layouts::De105Key,
        HandleControl::MapLettersToUnicode,
    ),
);

/// Process a raw scancode from the i8042 controller (PS/2 port 0x60).
/// Returns `Some(char)` if the scancode results in a printable character.
pub fn process_scancode(scancode: u8) -> Option<char> {
    let mut kb = KEYBOARD.lock();
    if let Ok(Some(key_event)) = kb.add_byte(scancode) {
        if let Some(key) = kb.process_keyevent(key_event) {
            return match key {
                DecodedKey::Unicode(c) => Some(c),
                DecodedKey::RawKey(_) => None, // function keys, arrows, etc.
            };
        }
    }
    None
}
