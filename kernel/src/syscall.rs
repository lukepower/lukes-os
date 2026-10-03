use x86_64::registers::model_specific::{Efer, EferFlags, Msr};

use crate::serial_println;
use crate::vfs;

// Model Specific Register addresses for x86_64 syscall/sysret
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;

// Syscall Numbers
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

// POSIX error codes (negative)
pub const ENOENT: i64 = -2;
pub const EFAULT: i64 = -14;
pub const EINVAL: i64 = -22;
pub const ENOSYS: i64 = -38;

pub const USER_ADDR_LIMIT: u64 = 0x0000_8000_0000_0000;

pub fn user_slice(ptr: u64, len: usize) -> Result<&'static [u8], i64> {
    if len == 0 {
        return Ok(&[]);
    }
    let end = ptr.checked_add(len as u64).ok_or(EFAULT)?;
    if end > USER_ADDR_LIMIT || ptr < 0x0040_0000 {
        return Err(EFAULT);
    }
    // Walk pages and ensure they are mapped and accessible
    let offset = crate::memory::PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
    let mapper = unsafe { crate::memory::init(x86_64::VirtAddr::new(offset)) };
    use x86_64::structures::paging::Translate;

    let start_page = x86_64::structures::paging::Page::<x86_64::structures::paging::Size4KiB>::containing_address(x86_64::VirtAddr::new(ptr));
    let end_page = x86_64::structures::paging::Page::<x86_64::structures::paging::Size4KiB>::containing_address(x86_64::VirtAddr::new(end - 1));

    for page in x86_64::structures::paging::Page::range_inclusive(start_page, end_page) {
        if mapper.translate_addr(page.start_address()).is_none() {
            return Err(EFAULT);
        }
    }

    Ok(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
}

pub fn user_slice_mut(ptr: u64, len: usize) -> Result<&'static mut [u8], i64> {
    if len == 0 {
        return Ok(&mut []);
    }
    let end = ptr.checked_add(len as u64).ok_or(EFAULT)?;
    if end > USER_ADDR_LIMIT || ptr < 0x0040_0000 {
        return Err(EFAULT);
    }
    let offset = crate::memory::PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
    let mapper = unsafe { crate::memory::init(x86_64::VirtAddr::new(offset)) };
    use x86_64::structures::paging::Translate;

    let start_page = x86_64::structures::paging::Page::<x86_64::structures::paging::Size4KiB>::containing_address(x86_64::VirtAddr::new(ptr));
    let end_page = x86_64::structures::paging::Page::<x86_64::structures::paging::Size4KiB>::containing_address(x86_64::VirtAddr::new(end - 1));

    for page in x86_64::structures::paging::Page::range_inclusive(start_page, end_page) {
        if mapper.translate_addr(page.start_address()).is_none() {
            return Err(EFAULT);
        }
    }

    Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len) })
}

/// Initialize the fast system call (SYSCALL / SYSRET) hardware extension.
pub fn init() {
    unsafe {
        // 1. Enable System Call Extensions (SCE) in EFER
        let mut flags = Efer::read();
        flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS);
        Efer::write(flags);

        // 2. Configure STAR register:
        // [63:48] = SYSRET CS and SS base selectors
        // SYSRET loads CS = selector + 16, SS = selector + 8
        // User data selector is 0x18 (index 3), User code selector is 0x20 (index 4)
        // [47:32] = SYSCALL CS and SS base selector
        // SYSCALL loads CS = selector, SS = selector + 8
        // Kernel code is 0x08, Kernel data is 0x10
        // Base for SYSRET = 0x10 | 3 = 0x13
        let star_val: u64 = ((0x13u64) << 48) | ((0x08u64) << 32);
        Msr::new(IA32_STAR).write(star_val);

        // 3. Configure LSTAR with address of syscall_entry assembly trampoline
        let lstar_addr = syscall_entry as *const () as u64;
        Msr::new(IA32_LSTAR).write(lstar_addr);

        // 4. Configure FMASK: clear IF (0x200), TF (0x100), DF (0x400) on syscall entry
        let fmask_val: u64 = 0x200 | 0x100 | 0x400;
        Msr::new(IA32_FMASK).write(fmask_val);
    }

    serial_println!("[OK] Fast SYSCALL / SYSRET MSRs initialized (EFER.SCE enabled)");
}

/// Assembly entry point invoked directly by hardware on `syscall` instruction.
#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        // On syscall:
        // RCX = user RIP (saved by hardware)
        // R11 = user RFLAGS (saved by hardware)
        // CS/SS loaded from STAR
        // RSP is STILL the user's RSP! We must switch to kernel stack.
        // Save user RSP in scratch, load kernel RSP from GS-base or PerCpu

        // Swap GS to access per-CPU kernel data
        "swapgs",
        // Temporarily store user RSP in GS-offset (we use GS:[16] as user RSP scratch)
        "mov gs:[16], rsp",
        // Load kernel syscall stack from GS:[8]
        "mov rsp, gs:[8]",

        // Push user context to kernel stack
        "push qword ptr gs:[16]", // user RSP
        "push r11",               // user RFLAGS
        "push rcx",               // user RIP

        // Push callee-saved registers
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Arguments according to standard x86_64 calling convention:
        // Syscall number: RAX -> passed in RDI
        // Arg 1: RDI -> passed in RSI
        // Arg 2: RSI -> passed in RDX
        // Arg 3: RDX -> passed in RCX
        // Arg 4: R10 -> passed in R8
        // Arg 5: R8  -> passed in R9
        "mov r9, r8",     // arg5
        "mov r8, r10",    // arg4
        "mov rcx, rdx",   // arg3
        "mov rdx, rsi",   // arg2
        "mov rsi, rdi",   // arg1
        "mov rdi, rax",   // syscall_num

        "call {syscall_dispatcher}",

        // Return value is in RAX. Preserve it while restoring state.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",

        // Pop user frame
        "pop rcx",               // user RIP
        "pop r11",               // user RFLAGS
        "pop rsp",               // restore user RSP

        // Restore user GS
        "swapgs",

        // Return to user mode (loads RIP from RCX, RFLAGS from R11)
        "sysretq",

        syscall_dispatcher = sym syscall_dispatcher,
    );
}

/// Unified Rust system call dispatcher.
#[no_mangle]
extern "C" fn syscall_dispatcher(
    num: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    _arg4: u64,
    _arg5: u64,
) -> i64 {
    match num {
        SYS_YIELD => {
            crate::scheduler::yield_now();
            0
        }
        SYS_EXIT => {
            serial_println!("[SYSCALL] Thread exit requested with code: {}", arg1);
            crate::scheduler::exit_current_thread();
        }
        SYS_WRITE => {
            // arg1: fd (1 = stdout, 2 = stderr)
            // arg2: ptr to buffer
            // arg3: len
            let fd = arg1;
            let ptr = arg2;
            let len = arg3 as usize;

            let slice = match user_slice(ptr, len) {
                Ok(s) => s,
                Err(err) => return err,
            };

            if fd == 1 || fd == 2 {
                if let Ok(s) = core::str::from_utf8(slice) {
                    crate::print!("{}", s);
                    crate::serial_print!("{}", s);
                    len as i64
                } else {
                    EINVAL
                }
            } else {
                EINVAL
            }
        }
        SYS_READ => {
            // arg1: fd (0 = stdin)
            // arg2: ptr to buffer
            // arg3: len
            let fd = arg1;
            let ptr = arg2;
            let len = arg3 as usize;

            let slice = match user_slice_mut(ptr, len) {
                Ok(s) => s,
                Err(err) => return err,
            };

            if fd == 0 && !slice.is_empty() {
                let ch = crate::keyboard::read_char();
                slice[0] = ch as u8;
                1
            } else {
                EINVAL
            }
        }
        SYS_GETPID => {
            let tid = crate::scheduler::current_thread_id();
            tid.0 as i64
        }
        SYS_UNAME => {
            // arg1: pointer to buffer
            let ptr = arg1;
            let len = arg2 as usize;
            let name = b"Luke's OS 0.2.0 (x86_64 SMP)";

            let slice = match user_slice_mut(ptr, len) {
                Ok(s) => s,
                Err(err) => return err,
            };

            if slice.len() >= name.len() {
                slice[..name.len()].copy_from_slice(name);
                name.len() as i64
            } else {
                EINVAL
            }
        }
        SYS_OPEN => {
            // arg1: path ptr, arg2: len, arg3: write (0=read, 1=write)
            let path_ptr = arg1;
            let path_len = arg2 as usize;

            let slice = match user_slice(path_ptr, path_len) {
                Ok(s) => s,
                Err(err) => return err,
            };

            if let Ok(path) = core::str::from_utf8(slice) {
                let flags = if arg3 == 1 {
                    vfs::OpenFlags::CREATE_OR_TRUNCATE
                } else {
                    vfs::OpenFlags::READ
                };
                match vfs::open(path, flags) {
                    Ok(_) => 3, // Dummy FD index for prototype
                    Err(_) => ENOENT,
                }
            } else {
                EINVAL
            }
        }
        SYS_SPAWN => {
            // arg1: path ptr, arg2: len
            let path_ptr = arg1;
            let path_len = arg2 as usize;

            let slice = match user_slice(path_ptr, path_len) {
                Ok(s) => s,
                Err(err) => return err,
            };

            if let Ok(path) = core::str::from_utf8(slice) {
                match crate::elf::spawn_user_process(path) {
                    Ok(tid) => tid.0 as i64,
                    Err(_) => ENOENT,
                }
            } else {
                EINVAL
            }
        }
        SYS_WAIT => {
            // arg1: pid
            // Wait for thread to exit
            let tid = crate::thread::ThreadId(arg1);
            loop {
                let threads = crate::scheduler::list_threads();
                if !threads.iter().any(|t| t.id == tid.0) {
                    break;
                }
                crate::scheduler::yield_now();
            }
            0
        }
        SYS_SLEEP => {
            // arg1: milliseconds
            let ms = arg1;
            let start_ticks = crate::interrupts::ticks();
            // LAPIC timer frequency is approx 100 Hz (1 tick = 10ms)
            let ticks_to_wait = (ms + 9) / 10;
            while crate::interrupts::ticks().saturating_sub(start_ticks) < ticks_to_wait {
                crate::scheduler::yield_now();
            }
            0
        }
        SYS_TIME => {
            // Returns milliseconds since boot
            let ticks = crate::interrupts::ticks();
            (ticks * 10) as i64
        }
        SYS_MMAP_ANON => {
            // arg1: length in bytes
            let len = arg1 as usize;
            if len == 0 || len > 32 * 1024 * 1024 {
                return EINVAL;
            }

            let pages_needed = (len + 4095) / 4096;
            let mut frame_guard = crate::memory::FRAME_ALLOCATOR.lock();
            let frame_allocator = match frame_guard.as_mut() {
                Some(fa) => fa,
                None => return ENOSYS,
            };

            // Allocate at top of user address space
            static NEXT_USER_MMAP: core::sync::atomic::AtomicU64 =
                core::sync::atomic::AtomicU64::new(0x0000_6000_0000_0000);
            let user_vaddr = NEXT_USER_MMAP.fetch_add((pages_needed as u64) * 4096, core::sync::atomic::Ordering::Relaxed);

            use x86_64::structures::paging::{Mapper, Page, PageTableFlags, Size4KiB};
            let offset = crate::memory::PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
            use x86_64::registers::control::Cr3;
            let (cr3_frame, _) = Cr3::read();
            let mut mapper = unsafe { crate::memory::page_table_for_frame(cr3_frame, x86_64::VirtAddr::new(offset)) };

            let start_page: Page<Size4KiB> = Page::containing_address(x86_64::VirtAddr::new(user_vaddr));
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

            for i in 0..pages_needed {
                let frame = match frame_allocator.allocate_frame_internal() {
                    Some(f) => f,
                    None => return -12, // ENOMEM
                };
                let page = start_page + i as u64;
                unsafe {
                    let _ = mapper.map_to(page, frame, flags, frame_allocator);
                }
            }

            user_vaddr as i64
        }
        _ => {
            serial_println!("[SYSCALL] Unknown syscall: {}", num);
            ENOSYS
        }
    }
}
