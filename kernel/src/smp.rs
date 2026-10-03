use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use x86_64::registers::control::Cr3;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::paging::{
    Mapper, OffsetPageTable, Page, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::apic;
use crate::gdt;
use crate::interrupts;
use crate::memory::{BootInfoFrameAllocator, PHYS_MEM_OFFSET};
use crate::serial_println;
use crate::sync::TicketLock;
use crate::thread::Thread;

pub const MAX_CPUS: usize = 8;
pub const TRAMPOLINE_PHYS_ADDR: u64 = 0x8000;
const IA32_GS_BASE_MSR: u32 = 0xC0000101;
const IA32_KERNEL_GS_BASE_MSR: u32 = 0xC0000102;

pub static CORES_ONLINE: AtomicUsize = AtomicUsize::new(1);
static AP_BOOT_LOCK: TicketLock<()> = TicketLock::new(());
static AP_READY_FLAG: AtomicBool = AtomicBool::new(false);

#[repr(C, align(64))]
pub struct PerCpu {
    pub self_ptr: *mut PerCpu,         // GS:[0]
    pub kernel_syscall_stack: u64,     // GS:[8]
    pub user_rsp_scratch: u64,         // GS:[16]
    pub core_id: usize,
    pub lapic_id: u8,
    pub active: bool,
    pub current_thread: Option<Box<Thread>>,
    pub idle_thread: Option<Box<Thread>>,
    pub run_queue: TicketLock<VecDeque<Box<Thread>>>,
    pub ticks: u64,
}

unsafe impl Send for PerCpu {}
unsafe impl Sync for PerCpu {}

static mut PER_CPU_DATA: [Option<PerCpu>; MAX_CPUS] = [
    None, None, None, None, None, None, None, None,
];

impl PerCpu {
    pub fn current() -> &'static mut PerCpu {
        let ptr: *mut PerCpu;
        unsafe {
            let msr = Msr::new(IA32_GS_BASE_MSR);
            ptr = msr.read() as *mut PerCpu;
            &mut *ptr
        }
    }

    pub fn get(core_id: usize) -> Option<&'static mut PerCpu> {
        if core_id < MAX_CPUS {
            unsafe { PER_CPU_DATA[core_id].as_mut() }
        } else {
            None
        }
    }
}

pub fn init_bsp(bsp_lapic_id: u8) {
    unsafe {
        let syscall_stack = alloc::vec![0u8; 4096 * 4].into_boxed_slice();
        let syscall_stack_top = (syscall_stack.as_ptr() as u64 + 4096 * 4) & !0xF;
        core::mem::forget(syscall_stack);

        let percpu = PerCpu {
            self_ptr: core::ptr::null_mut(),
            kernel_syscall_stack: syscall_stack_top,
            user_rsp_scratch: 0,
            core_id: 0,
            lapic_id: bsp_lapic_id,
            active: true,
            current_thread: Some(Box::new(Thread::bootstrap("bsp_boot", 0))),
            idle_thread: Some(Box::new(Thread::idle("bsp_idle", 0))),
            run_queue: TicketLock::new(VecDeque::new()),
            ticks: 0,
        };

        PER_CPU_DATA[0] = Some(percpu);
        let ptr = PER_CPU_DATA[0].as_mut().unwrap() as *mut PerCpu;
        (*ptr).self_ptr = ptr;

        let mut msr = Msr::new(IA32_GS_BASE_MSR);
        msr.write(ptr as u64);
        let mut kernel_gs_msr = Msr::new(IA32_KERNEL_GS_BASE_MSR);
        kernel_gs_msr.write(0);
    }
}

pub fn identity_map_trampoline(
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut BootInfoFrameAllocator,
) {
    let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(TRAMPOLINE_PHYS_ADDR));
    let frame: PhysFrame<Size4KiB> =
        PhysFrame::containing_address(PhysAddr::new(TRAMPOLINE_PHYS_ADDR));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    unsafe {
        if let Ok(mapping) = mapper.map_to(page, frame, flags, frame_allocator) {
            mapping.flush();
            serial_println!("[SMP] Trampoline page 0x8000 identity-mapped");
        } else {
            serial_println!("[SMP] Trampoline page 0x8000 already accessible");
        }
    }
}

fn write_trampoline_code() {
    let offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
    let trampoline_virt = (offset + TRAMPOLINE_PHYS_ADDR) as *mut u8;

    unsafe {
        // Zero first 512 bytes
        core::ptr::write_bytes(trampoline_virt, 0, 512);

        // 0x00: jmp short 0x50 (rel +0x4E)
        core::ptr::write(trampoline_virt.add(0x00), 0xEB);
        core::ptr::write(trampoline_virt.add(0x01), 0x4E);

        // 0x28: GDT Pointer: Limit = 31, Base = 0x8030
        let gdt_ptr: [u8; 6] = [0x1F, 0x00, 0x30, 0x80, 0x00, 0x00];
        core::ptr::copy_nonoverlapping(gdt_ptr.as_ptr(), trampoline_virt.add(0x28), 6);

        // 0x30: GDT Entries
        // Null (0x00)
        // 32-bit Code (0x08): 0x00CF9A00_0000FFFF
        let gdt_code32: [u8; 8] = [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x9A, 0xCF, 0x00];
        core::ptr::copy_nonoverlapping(gdt_code32.as_ptr(), trampoline_virt.add(0x38), 8);
        // 32-bit Data (0x10): 0x00CF9200_0000FFFF
        let gdt_data32: [u8; 8] = [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x92, 0xCF, 0x00];
        core::ptr::copy_nonoverlapping(gdt_data32.as_ptr(), trampoline_virt.add(0x40), 8);
        // 64-bit Code (0x18): 0x00AF9A00_00000000
        let gdt_code64: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x9A, 0xAF, 0x00];
        core::ptr::copy_nonoverlapping(gdt_code64.as_ptr(), trampoline_virt.add(0x48), 8);

        // 0x50: 16-bit Code
        let code16: [u8; 30] = [
            0xFA,                         // cli
            0x31, 0xC0,                   // xor ax, ax
            0x8E, 0xD8,                   // mov ds, ax
            0x8E, 0xC0,                   // mov es, ax
            0x8E, 0xD0,                   // mov ss, ax
            0xBC, 0x00, 0x7C,             // mov sp, 0x7c00
            0x0F, 0x01, 0x16, 0x28, 0x80, // lgdt [0x8028]
            0x0F, 0x20, 0xC0,             // mov eax, cr0
            0x0C, 0x01,                   // or al, 1
            0x0F, 0x22, 0xC0,             // mov cr0, eax
            0xEA, 0x70, 0x80, 0x08, 0x00, // jmp 0x08:0x8070
        ];
        core::ptr::copy_nonoverlapping(code16.as_ptr(), trampoline_virt.add(0x50), code16.len());

        // 0x70: 32-bit Code
        let code32: [u8; 61] = [
            0x66, 0xB8, 0x10, 0x00,       // mov ax, 0x10
            0x8E, 0xD8,                   // mov ds, ax
            0x8E, 0xC0,                   // mov es, ax
            0x8E, 0xD0,                   // mov ss, ax
            0x0F, 0x20, 0xE0,             // mov eax, cr4
            0x0D, 0xA0, 0x00, 0x00, 0x00, // or eax, 0xA0 (PAE | PGE)
            0x0F, 0x22, 0xE0,             // mov cr4, eax
            0xA1, 0x08, 0x80, 0x00, 0x00, // mov eax, [0x8008] (PML4)
            0x0F, 0x22, 0xD8,             // mov cr3, eax
            0xB9, 0x80, 0x00, 0x00, 0xC0, // mov ecx, 0xC0000080 (EFER)
            0x0F, 0x32,                   // rdmsr
            0x0D, 0x00, 0x09, 0x00, 0x00, // or eax, 0x900 (LME | NXE)
            0x0F, 0x30,                   // wrmsr
            0x0F, 0x20, 0xC0,             // mov eax, cr0
            0x0D, 0x00, 0x00, 0x00, 0x80, // or eax, 0x80000000 (PG)
            0x0F, 0x22, 0xC0,             // mov cr0, eax
            0xEA, 0xB0, 0x80, 0x00, 0x00, 0x18, 0x00, // jmp 0x18:0x80B0
        ];
        core::ptr::copy_nonoverlapping(code32.as_ptr(), trampoline_virt.add(0x70), code32.len());

        // 0xB0: 64-bit Code
        let code64: [u8; 26] = [
            0x48, 0x8B, 0x3C, 0x25, 0x20, 0x80, 0x00, 0x00, // mov rdi, [0x8020] (core_id)
            0x48, 0x8B, 0x24, 0x25, 0x18, 0x80, 0x00, 0x00, // mov rsp, [0x8018] (stack)
            0x48, 0x8B, 0x04, 0x25, 0x10, 0x80, 0x00, 0x00, // mov rax, [0x8010] (ap_entry)
            0xFF, 0xE0,                                     // jmp rax
        ];
        core::ptr::copy_nonoverlapping(code64.as_ptr(), trampoline_virt.add(0xB0), code64.len());
    }
}

pub fn boot_aps(ap_apic_ids: &[u8]) {
    write_trampoline_code();

    let offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
    let trampoline_virt = (offset + TRAMPOLINE_PHYS_ADDR) as *mut u8;

    let (pml4_frame, _) = Cr3::read();
    let pml4_phys = pml4_frame.start_address().as_u64();

    for (index, &apic_id) in ap_apic_ids.iter().enumerate() {
        let core_id = index + 1;
        if core_id >= MAX_CPUS {
            break;
        }

        let _guard = AP_BOOT_LOCK.lock();
        AP_READY_FLAG.store(false, Ordering::SeqCst);

        // Allocate a dedicated 64 KiB kernel boot stack for this AP
        let stack = alloc::vec![0u8; 4096 * 16].into_boxed_slice();
        let stack_top = (stack.as_ptr() as u64 + 4096 * 16) & !0xF;
        // Leak stack so it lives for the AP's lifetime
        core::mem::forget(stack);

        unsafe {
            // 0x8008: PML4 physical address
            core::ptr::write((trampoline_virt.add(0x08)) as *mut u64, pml4_phys);
            // 0x8010: AP 64-bit entry function
            core::ptr::write((trampoline_virt.add(0x10)) as *mut u64, ap_entry as *const () as usize as u64);
            // 0x8018: AP Boot Stack top
            core::ptr::write((trampoline_virt.add(0x18)) as *mut u64, stack_top);
            // 0x8020: Core ID
            core::ptr::write((trampoline_virt.add(0x20)) as *mut u64, core_id as u64);

            let ap_syscall_stack = alloc::vec![0u8; 4096 * 4].into_boxed_slice();
            let ap_syscall_stack_top = (ap_syscall_stack.as_ptr() as u64 + 4096 * 4) & !0xF;
            core::mem::forget(ap_syscall_stack);

            // Initialize PerCpu structure for this AP
            let percpu = PerCpu {
                self_ptr: core::ptr::null_mut(),
                kernel_syscall_stack: ap_syscall_stack_top,
                user_rsp_scratch: 0,
                core_id,
                lapic_id: apic_id,
                active: true,
                current_thread: Some(Box::new(Thread::bootstrap("ap_boot", core_id))),
                idle_thread: Some(Box::new(Thread::idle("ap_idle", core_id))),
                run_queue: TicketLock::new(VecDeque::new()),
                ticks: 0,
            };
            PER_CPU_DATA[core_id] = Some(percpu);
            let ptr = PER_CPU_DATA[core_id].as_mut().unwrap() as *mut PerCpu;
            (*ptr).self_ptr = ptr;
        }

        serial_println!("[SMP] Booting AP core_id={}, apic_id={}...", core_id, apic_id);

        // Send INIT IPI
        apic::send_init_ipi(apic_id);
        // Delay ~10ms
        for _ in 0..5_000_000 {
            core::hint::spin_loop();
        }

        // Send SIPI (vector 0x08 -> 0x8000)
        apic::send_sipi(apic_id, 0x08);

        // Wait up to 50ms for AP to boot
        let mut booted = false;
        for _ in 0..10_000_000 {
            if AP_READY_FLAG.load(Ordering::Acquire) {
                booted = true;
                break;
            }
            core::hint::spin_loop();
        }

        if !booted {
            // Retry SIPI once
            apic::send_sipi(apic_id, 0x08);
            for _ in 0..10_000_000 {
                if AP_READY_FLAG.load(Ordering::Acquire) {
                    booted = true;
                    break;
                }
                core::hint::spin_loop();
            }
        }

        if booted {
            serial_println!("[SMP] AP core {} online!", core_id);
        } else {
            serial_println!("[SMP] WARNING: AP core {} failed to respond to SIPI", core_id);
        }
    }
}

extern "C" fn ap_entry(core_id: u64) -> ! {
    let core_id = core_id as usize;

    // Load Per-CPU GDT and TSS
    gdt::init_cpu(core_id);
    // Load IDT
    interrupts::init_idt();

    // Enable FPU/SSE on AP
    crate::fpu::init();

    // Set IA32_GS_BASE to this core's PerCpu struct
    unsafe {
        let ptr = PER_CPU_DATA[core_id].as_mut().unwrap() as *mut PerCpu;
        let mut msr = Msr::new(IA32_GS_BASE_MSR);
        msr.write(ptr as u64);
        let mut kernel_gs_msr = Msr::new(IA32_KERNEL_GS_BASE_MSR);
        kernel_gs_msr.write(0);
    }

    // Initialize Local APIC on this AP
    apic::init_lapic();

    // Increment online counter and signal BSP
    CORES_ONLINE.fetch_add(1, Ordering::SeqCst);
    AP_READY_FLAG.store(true, Ordering::Release);

    serial_println!("[AP] CPU core {} initialized, entering scheduler idle loop", core_id);

    // Enable interrupts and enter scheduler loop
    x86_64::instructions::interrupts::enable();

    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}
