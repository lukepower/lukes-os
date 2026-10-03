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
            let ptr = arg2 as *const u8;
            let len = arg3 as usize;

            if ptr.is_null() || len == 0 {
                return 0;
            }

            // In user mode, safety check could be done via page tables
            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            if fd == 1 || fd == 2 {
                if let Ok(s) = core::str::from_utf8(slice) {
                    crate::print!("{}", s);
                    crate::serial_print!("{}", s);
                    len as i64
                } else {
                    -1
                }
            } else {
                -1 // Other FDs handled via VFS handle table if added
            }
        }
        SYS_READ => {
            // arg1: fd (0 = stdin)
            // arg2: ptr to buffer
            // arg3: len
            let fd = arg1;
            let ptr = arg2 as *mut u8;
            let len = arg3 as usize;

            if fd == 0 && !ptr.is_null() && len > 0 {
                let ch = crate::keyboard::read_char();
                unsafe {
                    *ptr = ch as u8;
                }
                1
            } else {
                -1
            }
        }
        SYS_GETPID => {
            let tid = crate::scheduler::current_thread_id();
            tid.0 as i64
        }
        SYS_UNAME => {
            // arg1: pointer to buffer
            let ptr = arg1 as *mut u8;
            let len = arg2 as usize;
            let name = b"Luke's OS 0.2.0 (x86_64 SMP)";
            if !ptr.is_null() && len >= name.len() {
                unsafe {
                    core::ptr::copy_nonoverlapping(name.as_ptr(), ptr, name.len());
                }
                name.len() as i64
            } else {
                -1
            }
        }
        SYS_OPEN => {
            // arg1: path ptr, arg2: len, arg3: write (0=read, 1=write)
            let path_ptr = arg1 as *const u8;
            let path_len = arg2 as usize;
            if path_ptr.is_null() {
                return -1;
            }
            let slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
            if let Ok(path) = core::str::from_utf8(slice) {
                let flags = if arg3 == 1 {
                    vfs::OpenFlags::CREATE_OR_TRUNCATE
                } else {
                    vfs::OpenFlags::READ
                };
                match vfs::open(path, flags) {
                    Ok(_) => 3, // Dummy FD index for prototype
                    Err(_) => -1,
                }
            } else {
                -1
            }
        }
        _ => {
            serial_println!("[SYSCALL] Unknown syscall: {}", num);
            -38 // ENOSYS
        }
    }
}
