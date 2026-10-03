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
}

pub static WM: Mutex<WindowManager> = Mutex::new(WindowManager {
    windows: Vec::new(),
    focused_window: None,
    next_id: 1,
    cursor_x: 200,
    cursor_y: 200,
    mouse_left_down: false,
    dragging_window: None,
});

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
                bb.put_pixel(px as usize, py as usize, fg);
            } else if let Some(bg_color) = bg {
                bb.put_pixel(px as usize, py as usize, bg_color);
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

    // 4. Draw Mouse Cursor
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
                'X' => bb.put_pixel(px as usize, py as usize, CURSOR_BORDER),
                '.' => bb.put_pixel(px as usize, py as usize, CURSOR_COLOR),
                _ => {}
            }
        }
    }

    // Mark entire screen dirty and present to front buffer
    bb.mark_dirty(Rect::new(0, 0, screen_w, screen_h));
    bb.present();
}

pub fn handle_input_events() {
    let mut wm = WM.lock();

    while let Some(event) = pop_event() {
        match event {
            InputEvent::MouseMove { dx, dy } => {
                let bb_width = BACKBUFFER.lock().as_ref().map(|b| b.width() as i32).unwrap_or(1280);
                let bb_height = BACKBUFFER.lock().as_ref().map(|b| b.height() as i32).unwrap_or(720);

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
                                wm.windows.remove(idx);
                                if wm.focused_window == Some(win_id) {
                                    wm.focused_window = wm.windows.last().map(|w| w.id);
                                }
                            } else {
                                // Raise window to top
                                let win = wm.windows.remove(idx);
                                wm.windows.push(win);
                                wm.focused_window = Some(win_id);

                                if clicked_titlebar {
                                    let win_ref = wm.windows.last().unwrap();
                                    let grab_x = mx - win_ref.rect.x;
                                    let grab_y = my - win_ref.rect.y;
                                    wm.dragging_window = Some((win_id, grab_x, grab_y));
                                }
                            }
                        } else {
                            // Check taskbar click
                            let bb_height = BACKBUFFER.lock().as_ref().map(|b| b.height() as i32).unwrap_or(720);
                            let taskbar_y = bb_height - TASKBAR_HEIGHT;
                            if my >= taskbar_y {
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
                    } else {
                        // Release drag
                        wm.dragging_window = None;
                    }
                }
            }
            InputEvent::Key { ch, pressed, .. } => {
                if pressed {
                    if let Some(c) = ch {
                        // Forward keystroke to focused console window (Terminal)
                        // If focused window is terminal (id == 1), push to keyboard buffer
                        if wm.focused_window == Some(1) {
                            crate::keyboard::push_char(c);
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
