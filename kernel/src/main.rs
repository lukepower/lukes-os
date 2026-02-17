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

use alloc::{string::String, vec, vec::Vec};
use bootloader_api::{config::Mapping, entry_point, BootInfo};
use core::panic::PanicInfo;
use x86_64::VirtAddr;

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

    serial_println!("========================================");
    serial_println!("  Luke's OS v0.1.0 — Booting...");
    serial_println!("========================================");

    // ── Framebuffer console ──
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        serial_println!("[OK] Framebuffer: {}x{} ({} bpp)",
            info.width, info.height, info.bytes_per_pixel * 8);
        vga::init(framebuffer);
    } else {
        serial_println!("[WARN] No framebuffer available — VGA output disabled");
    }

    println!("Luke's OS v0.1.0");

    // ── Memory ──
    let phys_mem_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("physical_memory_offset not available"),
    );

    // Initialize global physical memory offset for VirtIO HAL
    memory::PHYS_MEM_OFFSET.store(phys_mem_offset.as_u64(), core::sync::atomic::Ordering::Relaxed);

    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    
    // Initialize global frame allocator
    {
        let mut frame_allocator = memory::FRAME_ALLOCATOR.lock();
        *frame_allocator = Some(unsafe {
            memory::BootInfoFrameAllocator::init(&boot_info.memory_regions)
        });
    }

    // ── Heap ──
    {
        let mut frame_allocator_guard = memory::FRAME_ALLOCATOR.lock();
        let frame_allocator = frame_allocator_guard.as_mut().expect("frame allocator not initialized");
        allocator::init_heap(&mut mapper, frame_allocator)
            .expect("heap initialization failed");
    }

    serial_println!("[OK] Heap initialized ({} KiB)", allocator::HEAP_SIZE / 1024);

    // ── Verify alloc works ──
    let heap_test: Vec<u32> = vec![1, 2, 3, 4, 5];
    serial_println!("[OK] Heap allocation test: {:?}", heap_test);

    let greeting = String::from("Hello from Luke's OS kernel!");
    serial_println!("[OK] {}", greeting);
    println!("[OK] {}", greeting);

    // ── Filesystem (Phase 5.1) ──
    let ramfs = ramfs::RamFs::new();
    vfs::mount_root(ramfs.clone());
    serial_println!("[OK] RamFS mounted at /");

    // Create some directories
    let root = vfs::root().expect("Root FS not mounted");
    let tmp = root.mkdir("tmp").expect("failed to create /tmp");
    let _dev = root.mkdir("dev").expect("failed to create /dev");
    let _proc = root.mkdir("proc").expect("failed to create /proc");

    // Test file I/O
    let hello = tmp.create("hello.txt").expect("failed to create hello.txt");
    hello.write(0, b"Hello from RamFS!").expect("write failed");
    
    let mut buf = [0u8; 20];
    let bytes_read = hello.read(0, &mut buf).expect("read failed");
    let content = core::str::from_utf8(&buf[..bytes_read]).unwrap();
    serial_println!("[OK] Read from /tmp/hello.txt: '{}'", content);

    // ── PCI Enumeration (Phase 5.2) ──
    serial_println!("Scanning PCI bus...");
    pci::scan_bus(); // Just for printing, currently ignoring return

    // ── VirtIO Driver (Phase 5.3) ──
    virtio_blk::init();
    
    // verify read
    let driver = virtio_blk::VirtIoBlockDevice;
    use block::BlockDevice;
    
    let mut buf = [0u8; 512];
    // Need to handle error if driver not init
    match driver.read_block(0, &mut buf) {
        Ok(_) => {
             // Check first bytes
             let s = core::str::from_utf8(&buf[0..13]).unwrap_or("Inv UTF8");
             serial_println!("[OK] VirtIO Disk Read Sector 0: '{}'", s);
        }
        Err(e) => serial_println!("[WARN] VirtIO read failed: {:?}", e),
    }

    // ── Scheduler ──
    scheduler::init();
    serial_println!("[OK] Scheduler initialized");

    // Spawn demo threads
    scheduler::spawn("thread-A", || {
        for i in 0..10 {
            serial_println!("[Thread A] tick {}", i);
            println!("[Thread A] tick {}", i);
            scheduler::yield_now();
        }
        serial_println!("[Thread A] done");
    });

    scheduler::spawn("thread-B", || {
        for i in 0..10 {
            serial_println!("[Thread B] tick {}", i);
            println!("[Thread B] tick {}", i);
            scheduler::yield_now();
        }
        serial_println!("[Thread B] done");
    });

    // ── Enable interrupts ──
    x86_64::instructions::interrupts::enable();
    serial_println!("[OK] Interrupts enabled (timer + keyboard)");
    serial_println!("[OK] Luke's OS booted successfully!");
    serial_println!("========================================");

    // Idle loop — this is the boot thread (thread 0)
    // Check for pending reschedules after each hlt
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
        scheduler::schedule();
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
