// Thread and context definitions for preemptive multithreading.

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};

/// Unique thread identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadId(pub u64);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl ThreadId {
    pub fn new() -> Self {
        ThreadId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Saved CPU context for context switching.
/// Only callee-saved registers need preserving; the switch function
/// acts as a "call" boundary so caller-saved regs are handled by the compiler.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct Context {
    pub rsp: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rflags: u64,
    /// Instruction pointer — set to the return address for switch_context.
    pub rip: u64,
}

impl Context {
    pub const fn empty() -> Self {
        Context {
            rsp: 0,
            rbp: 0,
            rbx: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rflags: 0x200, // IF=1 (interrupts enabled)
            rip: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Dead,
}

/// Kernel stack size per thread (16 KiB).
const STACK_SIZE: usize = 4096 * 4;

pub struct Thread {
    pub id: ThreadId,
    pub name: &'static str,
    pub state: ThreadState,
    pub context: Context,
    /// Heap-allocated kernel stack (boxed so it's dropped when the thread dies).
    _stack: Option<Box<[u8]>>,
}

/// Trampoline that runs the thread entry function and then exits.
/// R15 contains the entry function pointer (set up in Thread::new).
extern "C" fn thread_entry_trampoline() -> ! {
    // Read entry function pointer from r15 (set in the thread's initial context)
    let entry_addr: u64;
    unsafe {
        core::arch::asm!("mov {}, r15", out(reg) entry_addr);
    }
    let func: fn() = unsafe { core::mem::transmute(entry_addr) };
    func();

    // When the thread function returns, mark it dead and yield
    crate::scheduler::exit_current_thread();
}

impl Thread {
    /// Create a new thread that will execute `entry` when scheduled.
    pub fn new(name: &'static str, entry: fn()) -> Self {
        let id = ThreadId::new();

        // Allocate kernel stack
        let stack = Box::new([0u8; STACK_SIZE]);
        let stack_top = stack.as_ptr() as u64 + STACK_SIZE as u64;

        // Set up the initial stack so that when switch_context "returns into"
        // this thread, it calls thread_entry_trampoline(entry).
        //
        // The x86_64 System V ABI passes the first argument in rdi.
        // We store the entry function address in rdi via the context setup.
        //
        // Stack layout (grows downward):
        //   [stack_top - 8]  = entry function pointer (will be loaded into rdi by trampoline)
        //   rsp points here, rip = thread_entry_trampoline
        //
        // Actually, for our switch_context we just set rip and rsp directly.
        // The trampoline reads its argument from r15 (we store entry there).

        let mut ctx = Context::empty();
        // rsp must be 16-byte aligned at function entry point per ABI
        ctx.rsp = (stack_top - 8) & !0xF;
        ctx.rip = thread_entry_trampoline as u64;
        ctx.r15 = entry as u64; // trampoline argument

        Thread {
            id,
            name,
            state: ThreadState::Ready,
            context: ctx,
            _stack: Some(stack),
        }
    }

    /// Create a "bootstrap" thread representing the currently running kernel code.
    /// Its context is empty and will be filled in on the first context switch.
    pub fn bootstrap(name: &'static str) -> Self {
        Thread {
            id: ThreadId(0),
            name,
            state: ThreadState::Running,
            context: Context::empty(),
            _stack: None, // uses the boot stack, not heap-allocated
        }
    }
}

/// Perform a context switch: save current context to `old`, load from `new`.
///
/// # Safety
/// Both pointers must be valid. Must be called with interrupts disabled.
#[unsafe(naked)]
pub unsafe extern "C" fn switch_context(old: *mut Context, new: *const Context) {
    // old: rdi, new: rsi (System V ABI)
    core::arch::naked_asm!(
        // Save callee-saved registers into old context
        "mov [rdi + 0x00], rsp",
        "mov [rdi + 0x08], rbp",
        "mov [rdi + 0x10], rbx",
        "mov [rdi + 0x18], r12",
        "mov [rdi + 0x20], r13",
        "mov [rdi + 0x28], r14",
        "mov [rdi + 0x30], r15",
        "pushfq",
        "pop rax",
        "mov [rdi + 0x38], rax",
        // Save return address as rip
        "lea rax, [rip + 2f]",
        "mov [rdi + 0x40], rax",

        // Load new context
        "mov rsp, [rsi + 0x00]",
        "mov rbp, [rsi + 0x08]",
        "mov rbx, [rsi + 0x10]",
        "mov r12, [rsi + 0x18]",
        "mov r13, [rsi + 0x20]",
        "mov r14, [rsi + 0x28]",
        "mov r15, [rsi + 0x30]",
        "mov rax, [rsi + 0x38]",
        "push rax",
        "popfq",

        // Jump to new thread (for new threads: trampoline; for resumed: return address)
        "mov rax, [rsi + 0x40]",
        "jmp rax",

        // Return point for threads that were switched away from
        "2:",
        "ret",
    );
}
