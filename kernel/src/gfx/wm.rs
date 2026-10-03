use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use crate::gfx::backbuffer::{BackBuffer, BACKBUFFER};
use crate::gfx::rect::Rect;
use crate::gfx::theme::*;
use crate::input::{pop_event, InputEvent};

pub const TITLE_BAR_HEIGHT: i32 = 24;
pub const BORDER_WIDTH: i32 = 1;
pub const TASKBAR_HEIGHT: i32 = 28;

pub static GUI_MODE: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WmEvent {
    pub kind: u32,
    pub a: i32,
    pub b: i32,
    pub c: u32,
}

pub mod event_kind {
    pub const KEY_DOWN: u32 = 1;
    pub const KEY_UP: u32 = 2;
    pub const CHAR: u32 = 3;
    pub const MOUSE_MOVE: u32 = 4;
    pub const MOUSE_DOWN: u32 = 5;
    pub const MOUSE_UP: u32 = 6;
    pub const SCROLL: u32 = 7;
    pub const RESIZE: u32 = 8;
    pub const CLOSE: u32 = 9;
    pub const FOCUS_IN: u32 = 10;
    pub const FOCUS_OUT: u32 = 11;
}

pub struct Window {
    pub id: u32,
    pub title: String,
    pub rect: Rect, // outer window rect including title bar and border
    pub z: u32,
    pub content: Vec<u32>, // pixel buffer of content area: (width - 2*BORDER) x (height - TITLE_BAR_HEIGHT - 2*BORDER)
    pub content_width: u32,
    pub content_height: u32,
    pub dirty: bool,
    pub closable: bool,
    pub owner_pid: Option<crate::process::ProcessId>,
    pub user_buffer_vaddr: u64,
    pub events: alloc::collections::VecDeque<WmEvent>,
}

impl Window {
    pub fn new(id: u32, title: String, rect: Rect, closable: bool) -> Self {
        let content_width = rect.width.saturating_sub((BORDER_WIDTH * 2) as u32);
        let content_height = rect.height.saturating_sub((TITLE_BAR_HEIGHT + BORDER_WIDTH * 2) as u32);
        let content_size = (content_width * content_height) as usize;
        let content = vec![WINDOW_BG; content_size];

        Self {
            id,
            title,
            rect,
            z: 0,
            content,
            content_width,
            content_height,
            dirty: true,
            closable,
            owner_pid: None,
            user_buffer_vaddr: 0,
            events: alloc::collections::VecDeque::new(),
        }
    }

    pub fn content_rect(&self) -> Rect {
        Rect::new(
            self.rect.x + BORDER_WIDTH,
            self.rect.y + TITLE_BAR_HEIGHT + BORDER_WIDTH,
            self.content_width,
            self.content_height,
        )
    }

    pub fn titlebar_rect(&self) -> Rect {
        Rect::new(
            self.rect.x,
            self.rect.y,
            self.rect.width,
            TITLE_BAR_HEIGHT as u32,
        )
    }

    pub fn close_btn_rect(&self) -> Rect {
        let size = 16;
        let pad = (TITLE_BAR_HEIGHT - size) / 2;
        Rect::new(
            self.rect.right() - pad - size,
            self.rect.y + pad,
            size as u32,
            size as u32,
        )
    }
}

pub struct WindowManager {
    pub windows: Vec<Window>,
    pub focused_window: Option<u32>,
    pub next_id: u32,
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub mouse_left_down: bool,
    pub dragging_window: Option<(u32, i32, i32)>, // (window_id, grab_offset_x, grab_offset_y)
    pub start_menu_open: bool,
    pub screen_width: i32,
    pub screen_height: i32,
}

pub static WM: Mutex<WindowManager> = Mutex::new(WindowManager {
    windows: Vec::new(),
    focused_window: None,
    next_id: 1,
    cursor_x: 200,
    cursor_y: 200,
    mouse_left_down: false,
    dragging_window: None,
    start_menu_open: false,
    screen_width: 1280,
    screen_height: 720,
});

pub fn init_screen_size(width: i32, height: i32) {
    let mut wm = WM.lock();
    wm.screen_width = width;
    wm.screen_height = height;
}

/// Create a new window managed by the WM
pub fn create_window(title: &str, x: i32, y: i32, width: u32, height: u32, closable: bool) -> u32 {
    let mut wm = WM.lock();
    let id = wm.next_id;
    wm.next_id += 1;

    let z = wm.windows.len() as u32;
    let mut win = Window::new(id, String::from(title), Rect::new(x, y, width, height), closable);
    win.z = z;

    wm.windows.push(win);
    wm.focused_window = Some(id);
    id
}

/// Create a window owned by a user process with a user-accessible buffer address
pub fn create_user_window(
    title: &str,
    width: u32,
    height: u32,
    owner_pid: crate::process::ProcessId,
    user_buffer_vaddr: u64,
) -> u32 {
    let mut wm = WM.lock();
    let id = wm.next_id;
    wm.next_id += 1;

    let z = wm.windows.len() as u32;
    // Position cascade: 60 + id*30
    let x = 60 + ((id as i32) * 20) % 400;
    let y = 60 + ((id as i32) * 20) % 250;
    let mut win = Window::new(id, String::from(title), Rect::new(x, y, width, height), true);
    win.z = z;
    win.owner_pid = Some(owner_pid);
    win.user_buffer_vaddr = user_buffer_vaddr;

    wm.windows.push(win);
    wm.focused_window = Some(id);
    id
}

pub fn destroy_window(win_id: u32) {
    let mut wm = WM.lock();
    if let Some(pos) = wm.windows.iter().position(|w| w.id == win_id) {
        wm.windows.remove(pos);
        if wm.focused_window == Some(win_id) {
            wm.focused_window = wm.windows.last().map(|w| w.id);
        }
    }
}

pub fn destroy_process_windows(pid: crate::process::ProcessId) {
    let mut wm = WM.lock();
    wm.windows.retain(|w| w.owner_pid != Some(pid));
    if let Some(fid) = wm.focused_window {
        if !wm.windows.iter().any(|w| w.id == fid) {
            wm.focused_window = wm.windows.last().map(|w| w.id);
        }
    }
}

pub fn get_window_buffer_vaddr(win_id: u32, pid: crate::process::ProcessId) -> Option<u64> {
    let wm = WM.lock();
    wm.windows.iter().find(|w| w.id == win_id && w.owner_pid == Some(pid)).map(|w| w.user_buffer_vaddr)
}

pub fn copy_to_window_content(
    win_id: u32,
    pid: crate::process::ProcessId,
    user_slice: &[u32],
    damage_x: u32,
    damage_y: u32,
    damage_w: u32,
    damage_h: u32,
) -> bool {
    let mut wm = WM.lock();
    if let Some(win) = wm.windows.iter_mut().find(|w| w.id == win_id && w.owner_pid == Some(pid)) {
        let cw = win.content_width as usize;
        let ch = win.content_height as usize;
        let total_pixels = cw * ch;
        if user_slice.len() < total_pixels {
            return false;
        }

        let x_end = (damage_x + damage_w).min(win.content_width) as usize;
        let y_end = (damage_y + damage_h).min(win.content_height) as usize;
        let x_start = damage_x as usize;
        let y_start = damage_y as usize;

        for y in y_start..y_end {
            let row_offset = y * cw;
            let src_row = &user_slice[row_offset + x_start..row_offset + x_end];
            let dst_row = &mut win.content[row_offset + x_start..row_offset + x_end];
            dst_row.copy_from_slice(src_row);
        }
        win.dirty = true;
        true
    } else {
        false
    }
}

pub fn poll_window_event(win_id: u32, pid: crate::process::ProcessId) -> Option<WmEvent> {
    let mut wm = WM.lock();
    if let Some(win) = wm.windows.iter_mut().find(|w| w.id == win_id && w.owner_pid == Some(pid)) {
        win.events.pop_front()
    } else {
        None
    }
}


/// Software mouse cursor bitmap (12x18 arrow)
const CURSOR_WIDTH: usize = 12;
const CURSOR_HEIGHT: usize = 18;
const CURSOR_MASK: [&str; 18] = [
    "X           ",
    "XX          ",
    "X.X         ",
    "X..X        ",
    "X...X       ",
    "X....X      ",
    "X.....X     ",
    "X......X    ",
    "X.......X   ",
    "X........X  ",
    "X.....XXXXX ",
    "X..X..X     ",
    "X.X X..X    ",
    "XX   X..X   ",
    "X     X..X  ",
    "      X..X  ",
    "       XX   ",
    "            ",
];

fn draw_glyph_8x16(bb: &mut BackBuffer, ch: char, x0: i32, y0: i32, fg: u32, bg: Option<u32>) {
    let glyph = crate::vga::font::glyph(ch);
    for (dy, &row) in glyph.iter().enumerate() {
        let py = y0 + dy as i32;
        if py < 0 || py >= bb.height() as i32 {
            continue;
        }
        for dx in 0..8 {
            let px = x0 + dx;
            if px < 0 || px >= bb.width() as i32 {
                continue;
            }
            let lit = (row >> (7 - dx)) & 1 != 0;
            if lit {
                bb.put_pixel_raw(px as usize, py as usize, fg);
            } else if let Some(bg_color) = bg {
                bb.put_pixel_raw(px as usize, py as usize, bg_color);
            }
        }
    }
}

fn draw_string(bb: &mut BackBuffer, s: &str, x: i32, y: i32, fg: u32, bg: Option<u32>) {
    let mut cur_x = x;
    for ch in s.chars() {
        draw_glyph_8x16(bb, ch, cur_x, y, fg, bg);
        cur_x += 8;
    }
}

pub fn render_frame(bb: &mut BackBuffer) {
    let screen_w = bb.width() as u32;
    let screen_h = bb.height() as u32;

    // 1. Draw Desktop background
    bb.fill_rect(Rect::new(0, 0, screen_w, screen_h), DESKTOP_BG);

    {
        let wm = WM.lock();
        let focused_id = wm.focused_window;

        // 2. Render Windows sorted by z-order
        for win in wm.windows.iter() {
            let is_focused = Some(win.id) == focused_id;
            let titlebar_color = if is_focused { TITLEBAR_ACTIVE } else { TITLEBAR_INACTIVE };

            // Outer border
            bb.fill_rect(win.rect, WINDOW_BORDER);

            // Titlebar
            let titlebar = win.titlebar_rect();
            bb.fill_rect(titlebar, titlebar_color);

            // Title text
            draw_string(bb, &win.title, titlebar.x + 8, titlebar.y + 4, TITLEBAR_TEXT, None);

            // Close button if closable
            if win.closable {
                let btn = win.close_btn_rect();
                bb.fill_rect(btn, BTN_CLOSE_BG);
                draw_string(bb, "X", btn.x + 4, btn.y + 1, 0x00FFFFFF, None);
            }

            // Window content
            let content_r = win.content_rect();
            bb.copy_rect(
                &win.content,
                win.content_width as usize,
                Rect::new(0, 0, win.content_width, win.content_height),
                content_r.x,
                content_r.y,
            );
        }

        // 3. Render Taskbar at bottom
        let taskbar_y = (screen_h - TASKBAR_HEIGHT as u32) as i32;
        let taskbar_rect = Rect::new(0, taskbar_y, screen_w, TASKBAR_HEIGHT as u32);
        bb.fill_rect(taskbar_rect, TASKBAR_BG);

        // "Luke's OS" Start button on taskbar
        let start_btn_rect = Rect::new(4, taskbar_y + 3, 100, TASKBAR_HEIGHT as u32 - 6);
        bb.fill_rect(start_btn_rect, TASKBAR_BTN_ACTIVE);
        draw_string(bb, "Luke's OS", start_btn_rect.x + 8, start_btn_rect.y + 3, 0x00FFFFFF, None);

        // Window tabs on taskbar
        let mut tab_x = start_btn_rect.right() + 8;
        for win in wm.windows.iter() {
            let is_focused = Some(win.id) == focused_id;
            let tab_bg = if is_focused { TASKBAR_BTN_ACTIVE } else { TASKBAR_BTN_BG };
            let tab_rect = Rect::new(tab_x, taskbar_y + 3, 120, TASKBAR_HEIGHT as u32 - 6);
            bb.fill_rect(tab_rect, tab_bg);

            // Truncate title if longer than 12 chars
            let display_title: String = win.title.chars().take(12).collect();
            draw_string(bb, &display_title, tab_rect.x + 6, tab_rect.y + 3, TASKBAR_TEXT, None);
            tab_x += 126;
        }

        // System uptime clock on right side of taskbar
        let ticks = crate::interrupts::ticks();
        let seconds = ticks / 100; // approximate
        let minutes = seconds / 60;
        let sec_rem = seconds % 60;
        let mut time_str = [b'0'; 5];
        time_str[0] = b'0' + ((minutes / 10) % 10) as u8;
        time_str[1] = b'0' + (minutes % 10) as u8;
        time_str[2] = b':';
        time_str[3] = b'0' + ((sec_rem / 10) % 10) as u8;
        time_str[4] = b'0' + (sec_rem % 10) as u8;
        if let Ok(clock_s) = core::str::from_utf8(&time_str) {
            let clock_x = screen_w as i32 - 60;
            draw_string(bb, clock_s, clock_x, taskbar_y + 6, TASKBAR_TEXT, None);
        }

        // 4. Render Start Menu popup if open
        if wm.start_menu_open {
            let menu_w = 140u32;
            let menu_h = 100u32;
            let menu_x = 4;
            let menu_y = taskbar_y - menu_h as i32;

            bb.fill_rect(Rect::new(menu_x, menu_y, menu_w, menu_h), 0x001E293B); // Dark slate
            // Border
            bb.fill_rect(Rect::new(menu_x, menu_y, menu_w, 1), 0x00334155);
            bb.fill_rect(Rect::new(menu_x, menu_y, 1, menu_h), 0x00334155);
            bb.fill_rect(Rect::new(menu_x + menu_w as i32 - 1, menu_y, 1, menu_h), 0x00334155);

            // Menu items: Clock, Paint, Files, About
            let items = [
                ("1. Clock", 0x00F8FAFC),
                ("2. Paint", 0x00F8FAFC),
                ("3. Files", 0x00F8FAFC),
                ("4. About", 0x0094A3B8),
            ];

            for (idx, (label, color)) in items.iter().enumerate() {
                let item_y = menu_y + 8 + (idx as i32 * 22);
                draw_string(bb, label, menu_x + 12, item_y, *color, None);
            }
        }

        // 5. Draw Mouse Cursor
        let cx = wm.cursor_x;
        let cy = wm.cursor_y;
        for (row_idx, line) in CURSOR_MASK.iter().enumerate() {
            let py = cy + row_idx as i32;
            if py < 0 || py >= screen_h as i32 {
                continue;
            }
            for (col_idx, ch) in line.chars().enumerate() {
                let px = cx + col_idx as i32;
                if px < 0 || px >= screen_w as i32 {
                    continue;
                }
                match ch {
                    'X' => bb.put_pixel_raw(px as usize, py as usize, CURSOR_BORDER),
                    '.' => bb.put_pixel_raw(px as usize, py as usize, CURSOR_COLOR),
                    _ => {}
                }
            }
        }
    }

    // Mark entire screen dirty and present to front buffer without holding WM.lock()
    bb.mark_dirty(Rect::new(0, 0, screen_w, screen_h));
    bb.present();
}

pub fn handle_input_events() {
    let mut wm = WM.lock();

    while let Some(event) = pop_event() {
        match event {
            InputEvent::MouseMove { dx, dy } => {
                let bb_width = wm.screen_width;
                let bb_height = wm.screen_height;

                wm.cursor_x = (wm.cursor_x + dx as i32).clamp(0, bb_width - 1);
                wm.cursor_y = (wm.cursor_y + dy as i32).clamp(0, bb_height - 1);

                let cx = wm.cursor_x;
                let cy = wm.cursor_y;

                // Handle dragging window
                if let Some((win_id, grab_x, grab_y)) = wm.dragging_window {
                    if let Some(win) = wm.windows.iter_mut().find(|w| w.id == win_id) {
                        win.rect.x = cx - grab_x;
                        win.rect.y = cy - grab_y;
                    }
                } else if let Some(fid) = wm.focused_window {
                    if let Some(win) = wm.windows.iter_mut().find(|w| w.id == fid) {
                        let c_rect = win.content_rect();
                        win.events.push_back(WmEvent {
                            kind: event_kind::MOUSE_MOVE,
                            a: cx - c_rect.x,
                            b: cy - c_rect.y,
                            c: 0,
                        });
                    }
                }
            }
            InputEvent::MouseButton { button, pressed } => {
                if button == 0 {
                    // Left click
                    wm.mouse_left_down = pressed;
                    if pressed {
                        let mx = wm.cursor_x;
                        let my = wm.cursor_y;

                        // Check hit on windows from top (highest z) to bottom
                        let mut clicked_win_idx = None;
                        let mut clicked_close = false;
                        let mut clicked_titlebar = false;

                        for (i, win) in wm.windows.iter().enumerate().rev() {
                            if win.closable && win.close_btn_rect().contains_point(mx, my) {
                                clicked_win_idx = Some(i);
                                clicked_close = true;
                                break;
                            } else if win.titlebar_rect().contains_point(mx, my) {
                                clicked_win_idx = Some(i);
                                clicked_titlebar = true;
                                break;
                            } else if win.rect.contains_point(mx, my) {
                                clicked_win_idx = Some(i);
                                break;
                            }
                        }

                        if let Some(idx) = clicked_win_idx {
                            let win_id = wm.windows[idx].id;
                            if clicked_close {
                                // Push CLOSE event to window
                                wm.windows[idx].events.push_back(WmEvent {
                                    kind: event_kind::CLOSE,
                                    a: 0,
                                    b: 0,
                                    c: 0,
                                });

                                // If this window is a kernel window (no owner_pid), remove it immediately.
                                // If owned by a user process, allow the process to receive CLOSE and call SYS_WIN_DESTROY or SYS_EXIT.
                                if wm.windows[idx].owner_pid.is_none() {
                                    wm.windows.remove(idx);
                                    if wm.focused_window == Some(win_id) {
                                        wm.focused_window = wm.windows.last().map(|w| w.id);
                                    }
                                }
                            } else {
                                // Raise window to top
                                let mut win = wm.windows.remove(idx);
                                win.events.push_back(WmEvent {
                                    kind: event_kind::FOCUS_IN,
                                    a: 0,
                                    b: 0,
                                    c: 0,
                                });
                                wm.windows.push(win);
                                wm.focused_window = Some(win_id);

                                if clicked_titlebar {
                                    let win_ref = wm.windows.last().unwrap();
                                    let grab_x = mx - win_ref.rect.x;
                                    let grab_y = my - win_ref.rect.y;
                                    wm.dragging_window = Some((win_id, grab_x, grab_y));
                                } else {
                                    // Click inside content area: dispatch MOUSE_DOWN
                                    let win_ref = wm.windows.last_mut().unwrap();
                                    let c_rect = win_ref.content_rect();
                                    if c_rect.contains_point(mx, my) {
                                        win_ref.events.push_back(WmEvent {
                                            kind: event_kind::MOUSE_DOWN,
                                            a: mx - c_rect.x,
                                            b: my - c_rect.y,
                                            c: 0,
                                        });
                                    }
                                }
                            }
                        } else {
                            // Check Start Menu click if open
                            let taskbar_y = wm.screen_height - TASKBAR_HEIGHT;

                            let mut clicked_menu_item = false;
                            if wm.start_menu_open {
                                let menu_w = 140u32;
                                let menu_h = 100u32;
                                let menu_rect = Rect::new(4, taskbar_y - menu_h as i32, menu_w, menu_h);
                                if menu_rect.contains_point(mx, my) {
                                    clicked_menu_item = true;
                                    let item_idx = (my - menu_rect.y) / 24;
                                    wm.start_menu_open = false;
                                    match item_idx {
                                        0 => {
                                            crate::scheduler::spawn("spawn-clock", || {
                                                let _ = crate::elf::spawn_user_process("/bin/clock");
                                            });
                                        }
                                        1 => {
                                            crate::scheduler::spawn("spawn-paint", || {
                                                let _ = crate::elf::spawn_user_process("/bin/paint");
                                            });
                                        }
                                        2 => {
                                            crate::scheduler::spawn("spawn-files", || {
                                                let _ = crate::elf::spawn_user_process("/bin/files");
                                            });
                                        }
                                        3 => {
                                            let _ = crate::gfx::wm::create_window("About Luke's OS", 200, 160, 360, 180, true);
                                        }
                                        _ => {}
                                    }
                                } else {
                                    wm.start_menu_open = false;
                                }
                            }

                            if !clicked_menu_item && my >= taskbar_y {
                                // Check Start button click
                                let start_btn = Rect::new(4, taskbar_y + 3, 100, TASKBAR_HEIGHT as u32 - 6);
                                if start_btn.contains_point(mx, my) {
                                    wm.start_menu_open = !wm.start_menu_open;
                                } else {
                                    let mut tab_x = 112;
                                    for win in wm.windows.iter() {
                                        let tab_rect = Rect::new(tab_x, taskbar_y + 3, 120, TASKBAR_HEIGHT as u32 - 6);
                                        if tab_rect.contains_point(mx, my) {
                                            wm.focused_window = Some(win.id);
                                            break;
                                        }
                                        tab_x += 126;
                                    }
                                }
                            }
                        }
                    } else {
                        // Release drag
                        wm.dragging_window = None;
                        let cur_x = wm.cursor_x;
                        let cur_y = wm.cursor_y;
                        if let Some(fid) = wm.focused_window {
                            if let Some(win) = wm.windows.iter_mut().find(|w| w.id == fid) {
                                let c_rect = win.content_rect();
                                win.events.push_back(WmEvent {
                                    kind: event_kind::MOUSE_UP,
                                    a: cur_x - c_rect.x,
                                    b: cur_y - c_rect.y,
                                    c: 0,
                                });
                            }
                        }
                    }
                }
            }
            InputEvent::Key { ch, code, pressed } => {
                if let Some(fid) = wm.focused_window {
                    if fid == 1 {
                        // Forward keystroke to Terminal window
                        if pressed {
                            if let Some(c) = ch {
                                crate::keyboard::push_char(c);
                            }
                        }
                    } else if let Some(win) = wm.windows.iter_mut().find(|w| w.id == fid) {
                        let kind = if pressed { event_kind::KEY_DOWN } else { event_kind::KEY_UP };
                        win.events.push_back(WmEvent {
                            kind,
                            a: code as i32,
                            b: ch.map(|c| c as i32).unwrap_or(0),
                            c: if pressed { 1 } else { 0 },
                        });
                        if pressed {
                            if let Some(c) = ch {
                                win.events.push_back(WmEvent {
                                    kind: event_kind::CHAR,
                                    a: c as i32,
                                    b: 0,
                                    c: 0,
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Window Manager main background thread loop
pub fn wm_thread_main() {
    crate::serial_println!("[OK] Window manager thread started");

    loop {
        if GUI_MODE.load(Ordering::Relaxed) {
            handle_input_events();

            if let Some(mut bb_guard) = BACKBUFFER.try_lock() {
                if let Some(ref mut bb) = bb_guard.as_mut() {
                    render_frame(bb);
                }
            }
        }

        crate::scheduler::yield_now();
    }
}
