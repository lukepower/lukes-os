use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::registers::model_specific::Msr;
use crate::memory::PHYS_MEM_OFFSET;

pub static LAPIC_PHYS_ADDR: AtomicU64 = AtomicU64::new(0xFEE00000);

const IA32_APIC_BASE_MSR: u32 = 0x1B;
const IA32_APIC_BASE_ENABLE: u64 = 1 << 11;

// LAPIC Register byte offsets
const REG_ID: usize = 0x020;
const REG_EOI: usize = 0x0B0;
const REG_TPR: usize = 0x080;
const REG_SVR: usize = 0x0F0;
const REG_ESR: usize = 0x280;
const REG_ICR_LOW: usize = 0x300;
const REG_ICR_HIGH: usize = 0x310;
const REG_LVT_TIMER: usize = 0x320;
const REG_TIMER_INIT_COUNT: usize = 0x380;
const REG_TIMER_CURR_COUNT: usize = 0x390;
const REG_TIMER_DIV_CONFIG: usize = 0x3E0;

pub const TIMER_INTERRUPT_VECTOR: u8 = 0x20;
pub const RESCHEDULE_IPI_VECTOR: u8 = 0xEC;
pub const SPURIOUS_INTERRUPT_VECTOR: u8 = 0xFF;

fn lapic_virt() -> *mut u32 {
    let phys = LAPIC_PHYS_ADDR.load(Ordering::Relaxed);
    let offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
    (offset + phys) as *mut u32
}

unsafe fn read_reg(reg: usize) -> u32 {
    let ptr = lapic_virt().add(reg / 4);
    core::ptr::read_volatile(ptr)
}

unsafe fn write_reg(reg: usize, value: u32) {
    let ptr = lapic_virt().add(reg / 4);
    core::ptr::write_volatile(ptr, value);
}

pub fn id() -> u8 {
    unsafe { ((read_reg(REG_ID) >> 24) & 0xFF) as u8 }
}

pub fn eoi() {
    unsafe { write_reg(REG_EOI, 0) };
}

pub fn init_lapic() {
    // 1. Ensure LAPIC is enabled via MSR
    let mut msr = Msr::new(IA32_APIC_BASE_MSR);
    let val = unsafe { msr.read() };
    if (val & IA32_APIC_BASE_ENABLE) == 0 {
        unsafe { msr.write(val | IA32_APIC_BASE_ENABLE) };
    }

    unsafe {
        // Clear Task Priority Register to accept all interrupts
        write_reg(REG_TPR, 0);

        // Clear Error Status Register
        write_reg(REG_ESR, 0);

        // Enable LAPIC in Spurious Vector Register (bit 8 = enable, bits 0-7 = vector)
        write_reg(REG_SVR, 0x100 | (SPURIOUS_INTERRUPT_VECTOR as u32));

        // Configure Local APIC Timer:
        // Divide configuration register: divide by 16
        write_reg(REG_TIMER_DIV_CONFIG, 0x03);

        // LVT Timer: Periodic mode (bit 17 = 1), vector = TIMER_INTERRUPT_VECTOR
        write_reg(REG_LVT_TIMER, (1 << 17) | (TIMER_INTERRUPT_VECTOR as u32));

        // Initial count: ~100 Hz timer tick (approx 10ms slice on typical QEMU clock)
        write_reg(REG_TIMER_INIT_COUNT, 0x0008_0000);
    }
}

pub fn send_ipi(dest_apic_id: u8, vector: u8, delivery_mode: u32) {
    unsafe {
        // Write destination APIC ID to ICR high (bits 24-31)
        write_reg(REG_ICR_HIGH, (dest_apic_id as u32) << 24);

        // Command: delivery mode (bits 8-10), Level Assert (bit 14), vector (bits 0-7)
        let cmd = (delivery_mode & 0x700) | (1 << 14) | (vector as u32);
        write_reg(REG_ICR_LOW, cmd);

        // Wait for delivery
        while (read_reg(REG_ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }
    }
}

pub fn send_init_ipi(dest_apic_id: u8) {
    // Delivery mode 5 = INIT
    send_ipi(dest_apic_id, 0, 5 << 8);
}

pub fn send_sipi(dest_apic_id: u8, page_number: u8) {
    // Delivery mode 6 = Startup (SIPI). Vector is the 4KiB page number
    send_ipi(dest_apic_id, page_number, 6 << 8);
}

pub fn send_reschedule_ipi(dest_apic_id: u8) {
    // Delivery mode 0 = Fixed
    send_ipi(dest_apic_id, RESCHEDULE_IPI_VECTOR, 0);
}
