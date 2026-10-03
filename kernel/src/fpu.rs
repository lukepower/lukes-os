use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

/// Initialize FPU/SSE support on the current CPU core.
/// Sets CR0.EM = 0, CR0.MP = 1, CR4.OSFXSR = 1, CR4.OSXMMEXCPT = 1, and runs fninit.
pub fn init() {
    unsafe {
        // Configure CR0: clear EM (bit 2), set MP (bit 1)
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
        Cr0::write(cr0);

        // Configure CR4: set OSFXSR (bit 9), OSXMMEXCPT (bit 10)
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR);
        cr4.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);

        // Initialize x87 FPU state
        core::arch::asm!("fninit");
    }
}
