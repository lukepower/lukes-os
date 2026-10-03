use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::apic;
use crate::smp::{CORES_ONLINE, MAX_CPUS, PerCpu};
use crate::sync::TicketLock;
use crate::thread::{Thread, ThreadId, ThreadState};

#[derive(Debug, Clone)]
pub struct ThreadInfo {
    pub id: u64,
    pub name: &'static str,
    pub state: ThreadState,
    pub core_id: usize,
}

static BLOCKED_THREADS: TicketLock<BTreeMap<ThreadId, Box<Thread>>> =
    TicketLock::new(BTreeMap::new());
static NEXT_SPAWN_CORE: AtomicUsize = AtomicUsize::new(0);

pub fn init() {}

pub fn spawn(name: &'static str, entry: fn()) {
    let mut thread = Box::new(Thread::new(name, entry));

    let num_cores = CORES_ONLINE.load(Ordering::Acquire).max(1);
    let target_core = NEXT_SPAWN_CORE.fetch_add(1, Ordering::Relaxed) % num_cores;
    thread.core_id = target_core;

    if let Some(target_percpu) = PerCpu::get(target_core) {
        target_percpu.run_queue.lock().push_back(thread);
        if target_core != 0 {
            apic::send_reschedule_ipi(target_percpu.lapic_id);
        }
    }
}

pub fn spawn_user(
    name: &'static str,
    entry: u64,
    user_stack_top: u64,
    process_id: crate::process::ProcessId,
) -> ThreadId {
    let mut thread = Box::new(Thread::new_user(name, entry, user_stack_top, process_id));
    let tid = thread.id;

    let num_cores = CORES_ONLINE.load(Ordering::Acquire).max(1);
    let target_core = NEXT_SPAWN_CORE.fetch_add(1, Ordering::Relaxed) % num_cores;
    thread.core_id = target_core;

    if let Some(target_percpu) = PerCpu::get(target_core) {
        target_percpu.run_queue.lock().push_back(thread);
        if target_core != 0 {
            apic::send_reschedule_ipi(target_percpu.lapic_id);
        }
    }
    tid
}

pub fn current_thread_id() -> ThreadId {
    let percpu = PerCpu::current();
    percpu
        .current_thread
        .as_ref()
        .map(|t| t.id)
        .unwrap_or(ThreadId(percpu.core_id as u64))
}

pub fn current_process_id() -> Option<crate::process::ProcessId> {
    let percpu = PerCpu::current();
    percpu
        .current_thread
        .as_ref()
        .and_then(|t| t.process_id)
}

pub fn block_current_thread() {
    let percpu = PerCpu::current();
    if let Some(cur) = percpu.current_thread.as_mut() {
        cur.state = ThreadState::Blocked;
    }
    yield_now();
}

pub fn unblock_thread(id: ThreadId) {
    if let Some(mut thread) = BLOCKED_THREADS.lock().remove(&id) {
        thread.state = ThreadState::Ready;
        let target_core = thread.core_id;
        if let Some(target_percpu) = PerCpu::get(target_core) {
            target_percpu.run_queue.lock().push_back(thread);
            apic::send_reschedule_ipi(target_percpu.lapic_id);
        }
    }
}

pub fn exit_current_thread() -> ! {
    let percpu = PerCpu::current();
    if let Some(cur) = percpu.current_thread.as_mut() {
        cur.state = ThreadState::Dead;
    }
    yield_now();
    loop {
        x86_64::instructions::hlt();
    }
}

pub fn yield_now() {
    unsafe {
        core::arch::asm!("int 0x20");
    }
}

pub fn schedule() {
    yield_now();
}

pub fn sleep(iterations: u64) {
    for _ in 0..iterations {
        yield_now();
    }
}

pub fn list_threads() -> Vec<ThreadInfo> {
    let mut list = Vec::new();
    let num_cores = CORES_ONLINE.load(Ordering::Acquire).min(MAX_CPUS);
    for core_id in 0..num_cores {
        if let Some(cpu) = PerCpu::get(core_id) {
            if let Some(cur) = &cpu.current_thread {
                list.push(ThreadInfo {
                    id: cur.id.0,
                    name: cur.name,
                    state: cur.state,
                    core_id,
                });
            }
            let guard = cpu.run_queue.lock();
            for t in guard.iter() {
                list.push(ThreadInfo {
                    id: t.id.0,
                    name: t.name,
                    state: t.state,
                    core_id,
                });
            }
        }
    }
    list
}

#[unsafe(naked)]
pub unsafe extern "C" fn timer_interrupt_asm() {
    core::arch::naked_asm!(
        // Check if coming from Ring 3 (saved CS at [rsp + 8] has bottom 2 bits != 0)
        "test qword ptr [rsp + 8], 3",
        "jz 1f",
        "swapgs",
        "1:",

        "push rax",
        "push rcx",
        "push rdx",
        "push rbx",
        "push rbp",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        "mov rdi, rsp",
        "call {schedule_tick}",
        "mov rsp, rax",

        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rbp",
        "pop rbx",
        "pop rdx",
        "pop rcx",
        "pop rax",

        // Check if returning to Ring 3 (saved CS at [rsp + 8] has bottom 2 bits != 0)
        "test qword ptr [rsp + 8], 3",
        "jz 2f",
        "swapgs",
        "2:",

        "iretq",
        schedule_tick = sym schedule_tick,
    );
}

#[no_mangle]
extern "C" fn schedule_tick(current_rsp: u64) -> u64 {
    apic::eoi();

    let percpu = PerCpu::current();
    percpu.ticks += 1;
    let my_core = percpu.core_id;

    if let Some(mut cur) = percpu.current_thread.take() {
        cur.saved_rsp = current_rsp;

        // Save FPU/SSE state for preempted thread
        unsafe {
            core::arch::asm!(
                "fxsave64 [{}]",
                in(reg) cur.fx_state.data.as_mut_ptr(),
            );
        }

        match cur.state {
            ThreadState::Running => {
                cur.state = ThreadState::Ready;
                percpu.run_queue.lock().push_back(cur);
            }
            ThreadState::Blocked | ThreadState::Sleeping => {
                BLOCKED_THREADS.lock().insert(cur.id, cur);
            }
            ThreadState::Dead => {}
            ThreadState::Ready => {
                percpu.run_queue.lock().push_back(cur);
            }
        }
    }

    // Work-Stealing: 1. Try local run queue
    let mut next_thread = percpu.run_queue.lock().pop_front();

    // 2. Work-stealing from other active cores
    if next_thread.is_none() {
        let active_cores = CORES_ONLINE.load(Ordering::Relaxed).min(MAX_CPUS);
        for victim_id in 0..active_cores {
            if victim_id != my_core {
                if let Some(victim) = PerCpu::get(victim_id) {
                    if let Some(mut guard) = victim.run_queue.try_lock() {
                        if let Some(mut stolen) = guard.pop_back() {
                            stolen.core_id = my_core;
                            next_thread = Some(stolen);
                            break;
                        }
                    }
                }
            }
        }
    }

    // 3. Fallback to idle thread
    let thread_to_run = match next_thread {
        Some(t) => t,
        None => percpu.idle_thread.take().unwrap_or_else(|| {
            Box::new(Thread::idle("idle_fallback", my_core))
        }),
    };

    // Restore FPU/SSE state for new thread
    unsafe {
        core::arch::asm!(
            "fxrstor64 [{}]",
            in(reg) thread_to_run.fx_state.data.as_ptr(),
        );
    }

    // CR3 page table switching:
    // If the next thread belongs to a user process, load its PML4; otherwise, keep/load kernel PML4.
    use x86_64::registers::control::{Cr3, Cr3Flags};
    if let Some(pid) = thread_to_run.process_id {
        if let Some(proc_arc) = crate::process::find_process(pid) {
            let pml4 = proc_arc.lock().pml4;
            let (current_cr3, _) = Cr3::read();
            if current_cr3 != pml4 {
                unsafe {
                    Cr3::write(pml4, Cr3Flags::empty());
                }
            }
        }
    }

    let next_rsp = thread_to_run.saved_rsp;
    if thread_to_run.kernel_stack_top != 0 {
        percpu.kernel_syscall_stack = thread_to_run.kernel_stack_top;
        crate::gdt::set_rsp0(my_core, x86_64::VirtAddr::new(thread_to_run.kernel_stack_top));
    }
    percpu.current_thread = Some(thread_to_run);

    next_rsp
}
