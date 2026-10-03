#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![allow(dead_code, unused_variables, static_mut_refs)]

extern crate alloc;

mod serial;
mod vga;
mod gdt;
mod interrupts;
mod keyboard;
mod memory;
mod allocator;
mod thread;
mod scheduler;
mod vfs;
mod ramfs;
mod lukefs;
mod pci;
mod block;
mod virtio_blk;
mod virtio_hal;
mod acpi;
mod apic;
mod sync;
mod smp;
mod shell;
mod syscall;
mod elf;
pub mod process;
pub mod fpu;
pub mod gfx;
pub mod mouse;
pub mod input;

use alloc::{string::String, vec, vec::Vec};
use bootloader_api::{config::Mapping, entry_point, BootInfo};
use core::panic::PanicInfo;
use x86_64::VirtAddr;

static SHARED_COUNTER: sync::SleepingMutex<u64> = sync::SleepingMutex::new(0);

/// Bootloader configuration: map all physical memory at an offset.
const CONFIG: bootloader_api::BootloaderConfig = {
    let mut config = bootloader_api::BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};

entry_point!(kernel_main, config = &CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // ── GDT & Interrupts ──
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::mask_pic_timer();
    fpu::init();

    serial_println!("========================================");
    serial_println!("  Luke's OS v0.2.0 — Booting SMP Kernel");
    serial_println!("========================================");

    // ── Framebuffer console ──
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        serial_println!(
            "[OK] Framebuffer: {}x{} ({} bpp)",
            info.width, info.height, info.bytes_per_pixel * 8
        );
        gfx::display::init(framebuffer);
        gfx::wm::init_screen_size(info.width as i32, info.height as i32);
        vga::init();
    } else {
        serial_println!("[WARN] No framebuffer available — VGA output disabled");
    }

    println!("Luke's OS v0.2.0 (SMP Edition)");

    // ── Memory ──
    let phys_mem_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("physical_memory_offset not available"),
    );

    // Initialize global physical memory offset for VirtIO HAL & ACPI
    memory::PHYS_MEM_OFFSET.store(phys_mem_offset.as_u64(), core::sync::atomic::Ordering::Relaxed);

    let mut mapper = unsafe { memory::init(phys_mem_offset) };

    // Initialize global frame allocator
    {
        let mut frame_allocator = memory::FRAME_ALLOCATOR.lock();
        *frame_allocator = Some(unsafe {
            memory::BootInfoFrameAllocator::init(&boot_info.memory_regions, phys_mem_offset.as_u64())
        });
    }

    // ── Heap ──
    {
        let mut frame_allocator_guard = memory::FRAME_ALLOCATOR.lock();
        let frame_allocator = frame_allocator_guard.as_mut().expect("frame allocator not initialized");
        allocator::init_heap(&mut mapper, frame_allocator)
            .expect("heap initialization failed");
    }

    // ── BackBuffer ──
    {
        let mut frame_guard = memory::FRAME_ALLOCATOR.lock();
        let frame_allocator = frame_guard.as_mut().expect("frame allocator missing");
        gfx::backbuffer::init(&mut mapper, frame_allocator);
    }

    // ── Memory diagnostics ──
    {
        let frame_allocator_guard = memory::FRAME_ALLOCATOR.lock();
        if let Some(fa) = frame_allocator_guard.as_ref() {
            serial_println!(
                "[OK] Physical Memory: {} MiB total ({} frames, {} free)",
                fa.total_memory_bytes() / (1024 * 1024),
                fa.total_frames(),
                fa.free_frames()
            );
        }
    }
    serial_println!(
        "[OK] Heap initialized ({} KiB total, {} KiB used, {} KiB free)",
        allocator::heap_size() / 1024,
        allocator::heap_used() / 1024,
        allocator::heap_free() / 1024
    );

    // ── Verify alloc works ──
    let heap_test: Vec<u32> = vec![1, 2, 3, 4, 5];
    serial_println!("[OK] Heap allocation test: {:?}", heap_test);

    let greeting = String::from("Hello from Luke's OS SMP kernel!");
    serial_println!("[OK] {}", greeting);
    println!("[OK] {}", greeting);

    // ── SMP Bringup: ACPI Discovery ──
    let mut secondary_cores = Vec::new();
    let mut bsp_lapic_id = 0;

    if let Some(rsdp_phys) = boot_info.rsdp_addr.into_option() {
        serial_println!("[OK] ACPI RSDP found at physical: 0x{:X}", rsdp_phys);
        if let Some(acpi_info) = acpi::parse_acpi(rsdp_phys) {
            apic::LAPIC_PHYS_ADDR.store(acpi_info.lapic_addr, core::sync::atomic::Ordering::Relaxed);
            serial_println!("[OK] LAPIC MMIO physical address: 0x{:X}", acpi_info.lapic_addr);

            // Read BSP LAPIC ID
            bsp_lapic_id = apic::id();
            serial_println!("[SMP] BSP Local APIC ID: {}", bsp_lapic_id);

            for cpu in acpi_info.cpus {
                if cpu.enabled && cpu.apic_id != bsp_lapic_id {
                    secondary_cores.push(cpu.apic_id);
                }
            }
        }
    } else {
        serial_println!("[WARN] RSDP not provided by bootloader");
    }

    // Initialize Local APIC on BSP
    apic::init_lapic();
    serial_println!("[OK] Local APIC initialized on BSP (timer enabled)");

    // Initialize BSP PerCpu and IA32_GS_BASE
    smp::init_bsp(bsp_lapic_id);
    serial_println!("[OK] PerCpu structure initialized on BSP (GS-base configured)");

    // Identity-map low-memory trampoline page at 0x8000
    {
        let mut frame_allocator_guard = memory::FRAME_ALLOCATOR.lock();
        let frame_allocator = frame_allocator_guard.as_mut().expect("frame allocator missing");
        smp::identity_map_trampoline(&mut mapper, frame_allocator);
    }

    // Boot secondary processor cores (APs)
    if !secondary_cores.is_empty() {
        serial_println!("[SMP] Detected {} secondary core(s). Starting AP bringup...", secondary_cores.len());
        smp::boot_aps(&secondary_cores);
        let online = smp::CORES_ONLINE.load(core::sync::atomic::Ordering::SeqCst);
        serial_println!("[SMP] All online CPU cores: {}", online);
        println!("[OK] SMP online: {} CPU cores active", online);
    }

    // ── PCI Enumeration & VirtIO Storage ──
    serial_println!("Scanning PCI bus...");
    pci::scan_bus();

    virtio_blk::init();

    // ── Filesystem (VFS) ──
    let ramfs = ramfs::RamFs::new();
    vfs::mount_root(ramfs);
    serial_println!("[OK] RamFS mounted at /");

    let _ = vfs::mkdir("/tmp");
    let _ = vfs::mkdir("/dev");
    let _ = vfs::mkdir("/proc");
    let _ = vfs::mkdir("/disk");
    let _ = vfs::mkdir("/bin");

    if let Ok(mut handle) = vfs::open("/tmp/hello.txt", vfs::OpenFlags::CREATE_OR_TRUNCATE) {
        let _ = handle.write(b"Hello from RamFS!");
        serial_println!("[OK] Created and wrote to /tmp/hello.txt");
    }

    // Embed and populate /bin user binaries
    static BIN_HELLO: &[u8] = include_bytes!("../../user/target/x86_64-unknown-none/release/hello");
    static BIN_CLOCK: &[u8] = include_bytes!("../../user/target/x86_64-unknown-none/release/clock");
    static BIN_PAINT: &[u8] = include_bytes!("../../user/target/x86_64-unknown-none/release/paint");
    static BIN_FILES: &[u8] = include_bytes!("../../user/target/x86_64-unknown-none/release/files");

    if let Ok(mut h) = vfs::open("/bin/hello", vfs::OpenFlags::CREATE_OR_TRUNCATE) {
        let _ = h.write(BIN_HELLO);
    }
    if let Ok(mut h) = vfs::open("/bin/clock", vfs::OpenFlags::CREATE_OR_TRUNCATE) {
        let _ = h.write(BIN_CLOCK);
    }
    if let Ok(mut h) = vfs::open("/bin/paint", vfs::OpenFlags::CREATE_OR_TRUNCATE) {
        let _ = h.write(BIN_PAINT);
    }
    if let Ok(mut h) = vfs::open("/bin/files", vfs::OpenFlags::CREATE_OR_TRUNCATE) {
        let _ = h.write(BIN_FILES);
    }
    serial_println!("[OK] Installed user binaries to /bin (hello, clock, paint, files)");

    // Attempt mounting persistent LukeFs on VirtIO disk
    let virtio_dev = alloc::sync::Arc::new(virtio_blk::VirtIoBlockDevice);
    match lukefs::LukeFs::mount(virtio_dev) {
        Ok(lfs) => {
            if let Ok(_) = vfs::mount("/disk", lfs) {
                serial_println!("[OK] LukeFs mounted persistently at /disk");
                println!("[OK] LukeFs mounted at /disk (VirtIO disk)");

                // Check or write a greeting file to persistent storage
                if let Ok(mut disk_file) = vfs::open("/disk/welcome.txt", vfs::OpenFlags::CREATE_OR_TRUNCATE) {
                    let _ = disk_file.write(b"Welcome to Luke's OS Persistent Storage on VirtIO!");
                    serial_println!("[OK] Wrote welcome message to /disk/welcome.txt");
                }
            }
        }
        Err(e) => {
            serial_println!("[WARN] Could not mount LukeFs on VirtIO device: {:?}", e);
        }
    }

    // ── Fast Syscall (SYSCALL/SYSRET) ──
    syscall::init();

    // ── Work-Stealing Scheduler ──
    scheduler::init();
    serial_println!("[OK] Work-stealing scheduler initialized");

    // ── PS/2 Mouse & Window Manager (M1) ──
    mouse::init();
    interrupts::unmask_mouse();

    // Create Terminal window (ID=1) for shell
    let _term_id = gfx::wm::create_window("Terminal", 40, 40, 640, 400, false);
    gfx::wm::GUI_MODE.store(true, core::sync::atomic::Ordering::Relaxed);

    // Initial frame render so desktop is visible immediately
    if let Some(mut bb_guard) = gfx::BACKBUFFER.try_lock() {
        if let Some(ref mut bb) = bb_guard.as_mut() {
            gfx::wm::render_frame(bb);
            serial_println!("[OK] Initial desktop frame rendered to screen");
        }
    }

    // ── Interactive Shell Thread ──
    scheduler::spawn("shell", || {
        shell::shell_main();
    });

    scheduler::spawn("wm", || {
        gfx::wm::wm_thread_main();
    });

    // ── Enable interrupts on BSP ──
    x86_64::instructions::interrupts::enable();
    serial_println!("[OK] Interrupts enabled on BSP (LAPIC timer ticking)");
    serial_println!("[OK] Luke's OS SMP kernel booted successfully!");
    serial_println!("========================================");

    // Idle loop — BSP core 0 idle thread
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}


#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("[PANIC] {}", info);
    println!("[PANIC] {}", info);
    loop {
        x86_64::instructions::hlt();
    }
}
