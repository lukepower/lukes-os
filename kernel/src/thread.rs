use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(pub u64);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl ThreadId {
    pub fn new() -> Self {
        ThreadId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Full saved CPU register state on an interrupt stack frame.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
    // Pushed by interrupt wrapper (in reverse order of push)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,

    // Hardware interrupt frame (pushed by CPU / iretq)
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Sleeping,
    Blocked,
    Dead,
}

/// Kernel stack size per thread (32 KiB).
pub const STACK_SIZE: usize = 4096 * 8;

pub struct Thread {
    pub id: ThreadId,
    pub name: &'static str,
    pub state: ThreadState,
    /// Stack pointer where Context is saved
    pub saved_rsp: u64,
    /// Preferred or current running CPU core
    pub core_id: usize,
    /// Heap-allocated kernel stack
    _stack: Option<Box<[u8]>>,
}

/// Trampoline that runs the thread entry function and exits.
/// R15 contains the entry function pointer.
extern "C" fn thread_entry_trampoline() -> ! {
    let entry_addr: u64;
    unsafe {
        core::arch::asm!("mov {}, r15", out(reg) entry_addr);
    }
    let func: fn() = unsafe { core::mem::transmute(entry_addr) };
    func();

    // Mark dead and yield
    crate::scheduler::exit_current_thread();
}

impl Thread {
    pub fn new(name: &'static str, entry: fn()) -> Self {
        let id = ThreadId::new();

        let stack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
        let stack_top = (stack.as_ptr() as u64 + STACK_SIZE as u64) & !0xF;

        // Reserve space for Context on top of the stack
        let ctx_size = core::mem::size_of::<Context>() as u64;
        let ctx_ptr = (stack_top - ctx_size) as *mut Context;

        let ctx = Context {
            r15: entry as u64,
            r14: 0,
            r13: 0,
            r12: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rdi: 0,
            rsi: 0,
            rbp: 0,
            rbx: 0,
            rdx: 0,
            rcx: 0,
            rax: 0,

            // Interrupt frame
            rip: thread_entry_trampoline as *const () as u64,
            cs: 0x08,      // Kernel code selector
            rflags: 0x202, // IF=1
            rsp: stack_top,
            ss: 0x10,      // Kernel data selector
        };

        unsafe {
            core::ptr::write(ctx_ptr, ctx);
        }

        Thread {
            id,
            name,
            state: ThreadState::Ready,
            saved_rsp: ctx_ptr as u64,
            core_id: 0,
            _stack: Some(stack),
        }
    }

    pub fn bootstrap(name: &'static str, core_id: usize) -> Self {
        Thread {
            id: ThreadId(core_id as u64),
            name,
            state: ThreadState::Running,
            saved_rsp: 0,
            core_id,
            _stack: None,
        }
    }

    pub fn idle(name: &'static str, core_id: usize) -> Self {
        fn idle_loop() {
            loop {
                x86_64::instructions::interrupts::enable_and_hlt();
            }
        }
        let mut t = Self::new(name, idle_loop);
        t.core_id = core_id;
        t
    }
}


