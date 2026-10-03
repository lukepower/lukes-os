#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

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
mod pci;
mod block;
mod virtio_blk;
mod virtio_hal;
mod acpi;
mod apic;
mod sync;
mod smp;

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
        vga::init(framebuffer);
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

    // ── Filesystem ──
    let ramfs = ramfs::RamFs::new();
    vfs::mount_root(ramfs.clone());
    serial_println!("[OK] RamFS mounted at /");

    let root = vfs::root().expect("Root FS not mounted");
    let tmp = root.mkdir("tmp").expect("failed to create /tmp");
    let _dev = root.mkdir("dev").expect("failed to create /dev");
    let _proc = root.mkdir("proc").expect("failed to create /proc");

    let hello = tmp.create("hello.txt").expect("failed to create hello.txt");
    hello.write(0, b"Hello from RamFS!").expect("write failed");

    let mut buf = [0u8; 20];
    let bytes_read = hello.read(0, &mut buf).expect("read failed");
    let content = core::str::from_utf8(&buf[..bytes_read]).unwrap();
    serial_println!("[OK] Read from /tmp/hello.txt: '{}'", content);

    // ── PCI Enumeration ──
    serial_println!("Scanning PCI bus...");
    pci::scan_bus();

    // ── VirtIO Driver ──
    virtio_blk::init();
    let driver = virtio_blk::VirtIoBlockDevice;
    use block::BlockDevice;
    let mut block_buf = [0u8; 512];
    match driver.read_block(0, &mut block_buf) {
        Ok(_) => {
            let s = core::str::from_utf8(&block_buf[0..13]).unwrap_or("Inv UTF8");
            serial_println!("[OK] VirtIO Disk Read Sector 0: '{}'", s);
        }
        Err(e) => serial_println!("[WARN] VirtIO read failed: {:?}", e),
    }

    // ── Work-Stealing Scheduler ──
    scheduler::init();
    serial_println!("[OK] Work-stealing scheduler initialized");

    // Spawn concurrent threads across cores demonstrating work-stealing and SleepingMutex
    scheduler::spawn("worker-A", || {
        for i in 0..5 {
            let core = smp::PerCpu::current().core_id;
            serial_println!("[Worker A | Core {}] tick {}", core, i);
            println!("[Worker A | Core {}] tick {}", core, i);
            {
                let mut guard = SHARED_COUNTER.lock();
                *guard += 10;
            }
            scheduler::yield_now();
        }
        serial_println!("[Worker A] finished successfully");
    });

    scheduler::spawn("worker-B", || {
        for i in 0..5 {
            let core = smp::PerCpu::current().core_id;
            serial_println!("[Worker B | Core {}] tick {}", core, i);
            println!("[Worker B | Core {}] tick {}", core, i);
            {
                let mut guard = SHARED_COUNTER.lock();
                *guard += 20;
            }
            scheduler::yield_now();
        }
        serial_println!("[Worker B] finished successfully");
    });

    scheduler::spawn("preempt-compute", || {
        let core = smp::PerCpu::current().core_id;
        serial_println!("[Compute Core {}] Starting intensive calculation...", core);
        let mut sum: u64 = 0;
        for i in 0..1_000_000 {
            sum = sum.wrapping_add(i);
        }
        serial_println!("[Compute Core {}] Calculation complete: sum={}", core, sum);
    });

    scheduler::spawn("counter-checker", || {
        let core = smp::PerCpu::current().core_id;
        for _ in 0..3 {
            let val = *SHARED_COUNTER.lock();
            serial_println!("[Checker | Core {}] Current SHARED_COUNTER={}", core, val);
            scheduler::yield_now();
        }
        serial_println!("[Checker] Finished");
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
