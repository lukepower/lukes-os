use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use x86_64::structures::paging::{PhysFrame, Size4KiB};

use crate::thread::ThreadId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

impl ProcessId {
    pub fn new() -> Self {
        ProcessId(NEXT_PID.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct Process {
    pub pid: ProcessId,
    pub name: String,
    pub pml4: PhysFrame<Size4KiB>,
    pub threads: Vec<ThreadId>,
    pub exit_code: Option<i32>,
}

unsafe impl Send for Process {}
unsafe impl Sync for Process {}

static PROCESS_TABLE: Mutex<Vec<Arc<Mutex<Process>>>> = Mutex::new(Vec::new());

pub fn register_process(process: Process) -> Arc<Mutex<Process>> {
    let proc_arc = Arc::new(Mutex::new(process));
    PROCESS_TABLE.lock().push(proc_arc.clone());
    proc_arc
}

pub fn find_process(pid: ProcessId) -> Option<Arc<Mutex<Process>>> {
    let table = PROCESS_TABLE.lock();
    table.iter().find(|p| p.lock().pid == pid).cloned()
}

pub fn remove_process(pid: ProcessId) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(pos) = table.iter().position(|p| p.lock().pid == pid) {
        let proc_arc = table.remove(pos);
        let pml4 = proc_arc.lock().pml4;
        let mut frame_guard = crate::memory::FRAME_ALLOCATOR.lock();
        if let Some(ref mut fa) = frame_guard.as_mut() {
            unsafe {
                crate::memory::free_user_page_table(pml4, fa);
            }
        }
    }
}
