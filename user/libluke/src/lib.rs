#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;
use spin::Mutex;

// Syscall constants matching kernel/src/syscall.rs & docs/ABI.md
pub const SYS_YIELD: u64 = 0;
pub const SYS_EXIT: u64 = 1;
pub const SYS_WRITE: u64 = 2;
pub const SYS_READ: u64 = 3;
pub const SYS_OPEN: u64 = 4;
pub const SYS_CLOSE: u64 = 5;
pub const SYS_GETPID: u64 = 6;
pub const SYS_UNAME: u64 = 7;
pub const SYS_SPAWN: u64 = 8;
pub const SYS_WAIT: u64 = 9;
pub const SYS_SLEEP: u64 = 10;
pub const SYS_TIME: u64 = 11;
pub const SYS_MMAP_ANON: u64 = 12;
pub const SYS_WIN_CREATE: u64 = 20;
pub const SYS_WIN_BUFFER: u64 = 21;
pub const SYS_WIN_PRESENT: u64 = 22;
pub const SYS_WIN_POLL: u64 = 23;
pub const SYS_WIN_DESTROY: u64 = 24;

#[inline(always)]
pub unsafe fn syscall0(num: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") num,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall1(num: u64, a1: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") num,
        in("rdi") a1,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall2(num: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") num,
        in("rdi") a1,
        in("rsi") a2,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") num,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall4(num: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") num,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall5(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") num,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
    );
    ret
}

// User-friendly syscall APIs
pub fn exit(code: u64) -> ! {
    unsafe {
        syscall1(SYS_EXIT, code);
    }
    loop {}
}

pub fn yield_now() {
    unsafe {
        syscall0(SYS_YIELD);
    }
}

pub fn getpid() -> u64 {
    unsafe { syscall0(SYS_GETPID) as u64 }
}

pub fn sleep(ms: u64) {
    unsafe {
        syscall1(SYS_SLEEP, ms);
    }
}

pub fn time() -> u64 {
    unsafe { syscall0(SYS_TIME) as u64 }
}

pub fn mmap_anon(len: usize) -> Result<*mut u8, i64> {
    let ret = unsafe { syscall1(SYS_MMAP_ANON, len as u64) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as *mut u8)
    }
}

pub fn write(fd: u64, buf: &[u8]) -> Result<usize, i64> {
    let ret = unsafe { syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as usize)
    }
}

pub fn read(fd: u64, buf: &mut [u8]) -> Result<usize, i64> {
    let ret = unsafe { syscall3(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as usize)
    }
}

pub fn open(path: &str, write_flag: bool) -> Result<u64, i64> {
    let ret = unsafe {
        syscall3(
            SYS_OPEN,
            path.as_ptr() as u64,
            path.len() as u64,
            if write_flag { 1 } else { 0 },
        )
    };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as u64)
    }
}

pub fn close(fd: u64) -> Result<(), i64> {
    let ret = unsafe { syscall1(SYS_CLOSE, fd) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(())
    }
}

pub fn spawn(path: &str) -> Result<u64, i64> {
    let ret = unsafe { syscall2(SYS_SPAWN, path.as_ptr() as u64, path.len() as u64) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as u64)
    }
}

pub fn wait(pid: u64) -> Result<i64, i64> {
    let ret = unsafe { syscall1(SYS_WAIT, pid) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret)
    }
}

// Window event representation
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
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

pub fn win_create(width: u32, height: u32, title: &str) -> Result<u32, i64> {
    let ret = unsafe {
        syscall4(
            SYS_WIN_CREATE,
            width as u64,
            height as u64,
            title.as_ptr() as u64,
            title.len() as u64,
        )
    };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as u32)
    }
}

pub fn win_buffer(win_id: u32) -> Result<*mut u32, i64> {
    let ret = unsafe { syscall1(SYS_WIN_BUFFER, win_id as u64) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as *mut u32)
    }
}

pub fn win_present(win_id: u32, x: u32, y: u32, w: u32, h: u32) -> Result<(), i64> {
    let ret = unsafe {
        syscall5(
            SYS_WIN_PRESENT,
            win_id as u64,
            x as u64,
            y as u64,
            w as u64,
            h as u64,
        )
    };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(())
    }
}

pub fn win_poll(win_id: u32) -> Option<Event> {
    let mut ev = Event {
        kind: 0,
        a: 0,
        b: 0,
        c: 0,
    };
    let ret = unsafe { syscall2(SYS_WIN_POLL, win_id as u64, &mut ev as *mut Event as u64) };
    if ret == 1 {
        Some(ev)
    } else {
        None
    }
}

pub fn win_destroy(win_id: u32) -> Result<(), i64> {
    let ret = unsafe { syscall1(SYS_WIN_DESTROY, win_id as u64) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(())
    }
}

// Print macros
pub struct Stdout;

impl core::fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let _ = write(1, s.as_bytes());
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        {
            use core::fmt::Write;
            let _ = write!($crate::Stdout, $($arg)*);
        }
    };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

// Bump / Chunk Allocator for userspace using SYS_MMAP_ANON
struct UserBumpAllocator {
    current: usize,
    end: usize,
}

impl UserBumpAllocator {
    const fn new() -> Self {
        Self { current: 0, end: 0 }
    }

    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();

        let alloc_start = (self.current + align - 1) & !(align - 1);
        let alloc_end = alloc_start.saturating_add(size);

        if alloc_end <= self.end && alloc_start != 0 {
            self.current = alloc_end;
            alloc_start as *mut u8
        } else {
            // Need more memory: request at least 64 KiB or requested size
            let chunk_size = core::cmp::max(64 * 1024, ((size + 4095) / 4096) * 4096);
            if let Ok(ptr) = mmap_anon(chunk_size) {
                self.current = ptr as usize;
                self.end = self.current + chunk_size;

                let real_start = (self.current + align - 1) & !(align - 1);
                self.current = real_start + size;
                real_start as *mut u8
            } else {
                core::ptr::null_mut()
            }
        }
    }
}

pub struct LockedUserAllocator(Mutex<UserBumpAllocator>);

impl LockedUserAllocator {
    pub const fn new() -> Self {
        Self(Mutex::new(UserBumpAllocator::new()))
    }
}

unsafe impl GlobalAlloc for LockedUserAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator does not reclaim individual allocations
    }
}

#[global_allocator]
static ALLOCATOR: LockedUserAllocator = LockedUserAllocator::new();

#[alloc_error_handler]
fn alloc_error_handler(_layout: Layout) -> ! {
    let _ = write(2, b"Out of memory error in user process!\n");
    exit(1);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("[USER PANIC] {}\n", info);
    exit(1);
}
