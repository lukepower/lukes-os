// Round-robin preemptive scheduler.
//
// Key design decisions:
// - The SCHEDULER lock is a spinlock; we MUST NOT hold it across context switches.
// - The timer IRQ uses try_lock() to avoid deadlock (single CPU).
// - Context pointers are extracted before dropping the lock.

use alloc::collections::VecDeque;
use crate::thread::{Thread, ThreadState, Context, switch_context};
use spin::Mutex;
use core::sync::atomic::{AtomicBool, Ordering};

static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);

/// Set by the timer IRQ; checked by schedule_if_needed().
static NEED_RESCHEDULE: AtomicBool = AtomicBool::new(false);

pub struct Scheduler {
    /// Currently running thread.
    current: Thread,
    /// Ready queue.
    ready: VecDeque<Thread>,
}

/// Initialize the scheduler with a bootstrap thread for the current (boot) context.
pub fn init() {
    let bootstrap = Thread::bootstrap("idle");
    *SCHEDULER.lock() = Some(Scheduler {
        current: bootstrap,
        ready: VecDeque::new(),
    });
}

/// Spawn a new kernel thread.
pub fn spawn(name: &'static str, entry: fn()) {
    let thread = Thread::new(name, entry);
    let mut guard = SCHEDULER.lock();
    if let Some(sched) = guard.as_mut() {
        sched.ready.push_back(thread);
    }
}

/// Called from the timer interrupt — just sets the reschedule flag.
/// We don't do the actual context switch here to avoid holding the
/// SCHEDULER spinlock across a context switch (which would deadlock).
pub fn on_timer_tick() {
    NEED_RESCHEDULE.store(true, Ordering::Release);
}

/// Perform the actual context switch if needed.
/// Called from safe points (e.g., enable_and_hlt wrapper, yield_now).
/// Interrupts should be disabled when calling this.
fn do_schedule() {
    // Pointers we need for the switch — extracted while holding the lock
    let (old_ctx_ptr, new_ctx_ptr): (*mut Context, *const Context);

    {
        let mut guard = SCHEDULER.lock();
        let sched = match guard.as_mut() {
            Some(s) => s,
            None => return,
        };

        if sched.ready.is_empty() {
            return;
        }

        let mut next = match sched.ready.pop_front() {
            Some(t) => t,
            None => return,
        };

        next.state = ThreadState::Running;
        sched.current.state = ThreadState::Ready;

        // Swap
        let old = core::mem::replace(&mut sched.current, next);
        sched.ready.push_back(old);

        // Get raw pointers
        let old_thread = sched.ready.back_mut().unwrap();
        old_ctx_ptr = &mut old_thread.context as *mut Context;
        new_ctx_ptr = &sched.current.context as *const Context;

        // Lock is dropped here
    }

    // Context switch happens WITHOUT the scheduler lock held
    unsafe {
        switch_context(old_ctx_ptr, new_ctx_ptr);
    }
}

/// Called by threads to check if a reschedule is pending and perform it.
/// This is the safe entry point for preemptive scheduling.
pub fn schedule() {
    if NEED_RESCHEDULE.swap(false, Ordering::Acquire) {
        x86_64::instructions::interrupts::without_interrupts(|| {
            do_schedule();
        });
    }
}

/// Mark the current thread as dead and switch to the next ready thread.
/// Called when a thread's entry function returns.
pub fn exit_current_thread() -> ! {
    loop {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut guard = SCHEDULER.lock();
            if let Some(sched) = guard.as_mut() {
                sched.current.state = ThreadState::Dead;

                if let Some(mut next) = sched.ready.pop_front() {
                    next.state = ThreadState::Running;
                    let _dead = core::mem::replace(&mut sched.current, next);

                    let new_ctx_ptr = &sched.current.context as *const Context;
                    drop(guard);

                    let mut dummy = Context::empty();
                    unsafe {
                        switch_context(&mut dummy as *mut _, new_ctx_ptr);
                    }
                }
            }
        });
        x86_64::instructions::hlt();
    }
}

/// Voluntarily yield the current thread's time slice.
pub fn yield_now() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        do_schedule();
    });
}
