use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use x86_64::VirtAddr;
use lazy_static::lazy_static;
use crate::gdt;
use crate::apic;
use crate::serial_println;
use pic8259::ChainedPics;
use spin;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
    Mouse = (PIC_2_OFFSET + 4),
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }

        // Local APIC Timer Preemptive ISR
        unsafe {
            idt[apic::TIMER_INTERRUPT_VECTOR]
                .set_handler_addr(VirtAddr::new(crate::scheduler::timer_interrupt_asm as *const () as usize as u64));
            idt[apic::RESCHEDULE_IPI_VECTOR]
                .set_handler_addr(VirtAddr::new(crate::scheduler::timer_interrupt_asm as *const () as usize as u64));
        }

        idt[InterruptIndex::Keyboard.as_u8()].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Mouse.as_u8()].set_handler_fn(mouse_interrupt_handler);
        idt[apic::SPURIOUS_INTERRUPT_VECTOR].set_handler_fn(spurious_interrupt_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

static TICK_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn ticks() -> u64 {
    TICK_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn tick() {
    TICK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

pub fn mask_pic_timer() {
    unsafe {
        // Mask IRQ 0 (timer) on master PIC (port 0x21), keep IRQ 1 (keyboard) unmasked
        let mut port = x86_64::instructions::port::Port::<u8>::new(0x21);
        let mask = port.read();
        port.write(mask | 0x01); // Mask bit 0 (IRQ 0)
    }
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial_println!("[EXCEPTION] BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("[EXCEPTION] DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: x86_64::structures::idt::PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    serial_println!("[EXCEPTION] PAGE FAULT");
    serial_println!("Accessed Address: {:?}", Cr2::read());
    serial_println!("Error Code: {:?}", error_code);
    serial_println!("{:#?}", stack_frame);
    panic!("PAGE FAULT — cannot continue");
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // Spurious interrupts do not require an EOI
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    let ch = crate::keyboard::process_scancode(scancode);
    if !crate::gfx::wm::GUI_MODE.load(core::sync::atomic::Ordering::Relaxed) {
        if let Some(c) = ch {
            crate::keyboard::push_char(c);
        }
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    crate::mouse::handle_interrupt();

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Mouse.as_u8());
    }
}

pub fn unmask_mouse() {
    unsafe {
        // Master PIC: unmask IRQ2 (cascade)
        let mut master_port = x86_64::instructions::port::Port::<u8>::new(0x21);
        let master_mask = master_port.read();
        master_port.write(master_mask & !(1 << 2));

        // Slave PIC: unmask IRQ12 (bit 4 on slave PIC, port 0xA1)
        let mut slave_port = x86_64::instructions::port::Port::<u8>::new(0xA1);
        let slave_mask = slave_port.read();
        slave_port.write(slave_mask & !(1 << 4));
    }
}

/// Read current time from CMOS RTC: returns (hours, minutes, seconds).
/// Handles BCD and 12/24 hour formats.
pub fn read_rtc_time() -> (u8, u8, u8) {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut cmos_addr = Port::<u8>::new(0x70);
        let mut cmos_data = Port::<u8>::new(0x71);

        let read_register = |reg: u8, addr: &mut Port<u8>, data: &mut Port<u8>| -> u8 {
            addr.write(reg);
            data.read()
        };

        // Wait until RTC update is not in progress (Register A, bit 7)
        loop {
            let status_a = read_register(0x0A, &mut cmos_addr, &mut cmos_data);
            if (status_a & 0x80) == 0 {
                break;
            }
        }

        let mut sec = read_register(0x00, &mut cmos_addr, &mut cmos_data);
        let mut min = read_register(0x02, &mut cmos_addr, &mut cmos_data);
        let mut hour = read_register(0x04, &mut cmos_addr, &mut cmos_data);
        let register_b = read_register(0x0B, &mut cmos_addr, &mut cmos_data);

        // Convert BCD to binary if bit 2 of Register B is 0
        let is_bcd = (register_b & 0x04) == 0;
        if is_bcd {
            sec = ((sec >> 4) * 10) + (sec & 0x0F);
            min = ((min >> 4) * 10) + (min & 0x0F);
            hour = (((hour & 0x70) >> 4) * 10) + (hour & 0x0F) | (hour & 0x80);
        }

        // Convert 12 hour to 24 hour if needed (bit 1 of Register B is 0)
        let is_24h = (register_b & 0x02) != 0;
        if !is_24h && (hour & 0x80) != 0 {
            hour = ((hour & 0x7F) + 12) % 24;
        }

        (hour % 24, min % 60, sec % 60)
    }
}

