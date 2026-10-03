# Luke's OS

<p align="center">
  <img src="resources/img/lukes_os_logo.png" alt="Luke's OS Logo" width="480">
</p>

**Luke's OS** is an educational, 64-bit x86 symmetric multiprocessing (SMP) hobby operating system developed in **Rust**. It explores modern operating system architecture principles — from bare-metal bootstrapping and multi-core AP bringup to high-performance memory allocators, preemptive multitasking, PCI enumeration, VirtIO block storage, and virtual file systems.

---

## Architecture & Subsystems

```
                                +---------------------------+
                                |  Luke's OS (SMP Edition)  |
                                +---------------------------+
                                              |
        +------------------+------------------+------------------+------------------+
        |                  |                  |                  |                  |
+---------------+  +---------------+  +---------------+  +---------------+  +---------------+
|  Multi-Core   |  | Memory Engine |  | Multitasking  |  | Console & IO  |  | Storage & VFS |
+---------------+  +---------------+  +---------------+  +---------------+  +---------------+
| • ACPI (MADT) |  | • O(1) Frames |  | • Preemptive  |  | • Framebuffer |  | • PCI Scanner |
| • LAPIC + IPI |  | • Contig. DMA |  |   Timer ISR   |  |   Text Console|  | • VirtIO-Blk  |
| • AP Tramp.   |  | • 4 MiB Heap  |  | • TicketLock  |  | • 16550 Serial|  | • VFS / RamFS |
| • Per-Core GDT|  | • Dynamic Grow|  | • Sleep Mutex |  | • PS/2 Keys   |  | • Sector I/O  |
+---------------+  +---------------+  +---------------+  +---------------+  +---------------+
```

### 1. Bootstrapping & Symmetric Multiprocessing (SMP)
- **Bootloader**: Built on `bootloader_api` (v0.11), supporting both **BIOS** (`rustos-bios.img`) and **UEFI** (`rustos-uefi.img`) boot modes.
- **ACPI Table Parser (`kernel/src/acpi.rs`)**: Scans RSDP and MADT (Multiple APIC Description Table) to discover Local APIC addresses and enumerate online CPU cores.
- **Local APIC (`kernel/src/apic.rs`)**: MSR-enabled LAPIC managing timer interrupts, spurious interrupts, and Inter-Processor Interrupts (IPIs).
- **AP Bringup Trampoline (`kernel/src/smp.rs`)**: Broadcasts INIT-SIPI-SIPI sequences to transition secondary cores from 16-bit real mode into 64-bit long mode via a low-memory trampoline page (`0x8000`).
- **Per-CPU Tracking**: Configures per-core GDT/TSS structures and `IA32_GS_BASE` for thread-local core state.

### 2. Memory Management Engine
- **O(1) Physical Frame Allocator (`kernel/src/memory.rs`)**:
  - Replaces linear scans with an intrusive free list and multi-region bump allocator.
  - Supports **contiguous multi-frame allocation** (`allocate_contiguous`) required for hardware DMA buffers.
  - Implements full frame deallocation (`deallocate_frame`, `deallocate_contiguous`).
  - Real-time diagnostics: `total_memory_bytes()`, `free_memory_bytes()`, `allocated_frames()`.
- **VirtIO DMA HAL (`kernel/src/virtio_hal.rs`)**: Zero-leak DMA allocator backed by contiguous frame allocations.
- **Dynamic Kernel Heap (`kernel/src/allocator.rs`)**:
  - Initial 4 MiB heap mapped at virtual address `0x4444_4444_0000`.
  - Supports runtime dynamic extension via `extend_heap(mapper, frame_allocator, bytes)`.
  - Diagnostics: `heap_size()`, `heap_used()`, `heap_free()`.

### 3. Synchronization & Multitasking
- **TicketLock (`kernel/src/sync.rs`)**: Fair FIFO ticket spinlock preventing core starvation across multi-core workloads.
- **SleepingMutex & WaitQueue (`kernel/src/sync.rs`)**: Blocking synchronization primitives that deschedule contending threads.
- **Preemptive Scheduler (`kernel/src/scheduler.rs`, `kernel/src/thread.rs`)**:
  - Interrupt-driven preemption via `timer_interrupt_asm` capturing full register state.
  - Safe deferred zombie reaping (`sched.dead`), preventing stack use-after-free on thread termination.
  - Sleeping thread queues via `scheduler::sleep(ticks)`.

### 4. Display & Serial Output
- **Framebuffer Console (`kernel/src/vga.rs`)**: Pixel-rendered text console with custom 8x16 font, screen scrolling, ASCII, and extended European characters (`ä`, `ö`, `ü`, `ß`, `€`, `°`).
- **Serial Logger (`kernel/src/serial.rs`)**: COM1 UART logger (`0x3F8`) allowing headless debugging and automated test logs via `serial_println!`.

### 5. Storage & Virtual File System (VFS)
- **PCI Scanner (`kernel/src/pci.rs`)**: Configuration-space scanner probing bus 0 for devices.
- **VirtIO Block Driver (`kernel/src/virtio_blk.rs`)**: Block device driver built on `virtio-drivers 0.12`.
- **VFS & RamFS (`kernel/src/vfs.rs`, `kernel/src/ramfs.rs`)**: `Inode` and `FileSystem` traits with an in-memory RamFS mounted at `/`.

---

## Project Structure

```
lukes-os/
├── .cargo/
│   └── config.toml          # build-std flags, target configuration & MSVC env
├── kernel/                  # Bare-metal x86_64 SMP kernel (no_std)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs          # Kernel entry point and SMP bringup sequence
│       ├── acpi.rs          # ACPI RSDP & MADT table parser
│       ├── allocator.rs     # 4 MiB dynamic heap allocator & diagnostics
│       ├── apic.rs          # Local APIC driver & IPI handling
│       ├── block.rs         # Unified BlockDevice trait
│       ├── gdt.rs           # Per-core GDT, TSS, and IST setup
│       ├── interrupts.rs    # IDT handlers, LAPIC timer vector, PIC mask
│       ├── keyboard.rs      # PS/2 scancode decoder
│       ├── memory.rs        # O(1) contiguous frame allocator & paging
│       ├── pci.rs           # PCI bus scanner
│       ├── ramfs.rs         # In-memory filesystem implementation
│       ├── scheduler.rs     # Preemptive timer ISR & scheduler
│       ├── serial.rs        # COM1 16550 UART driver & macros
│       ├── smp.rs           # AP trampoline, PerCpu, and multi-core boot
│       ├── sync.rs          # TicketLock, WaitQueue & SleepingMutex
│       ├── thread.rs        # Thread state, stack context & switch_context
│       ├── vfs.rs           # VFS abstraction traits (Inode, FileSystem)
│       ├── vga.rs           # Framebuffer text console with bitmap font
│       ├── virtio_blk.rs    # VirtIO block device driver
│       └── virtio_hal.rs    # DMA HAL for virtio-drivers
├── runner/                  # Host launcher and disk packaging tool
│   ├── build.rs             # Builds BIOS/UEFI images via bootloader crate
│   ├── Cargo.toml
│   └── src/
│       └── main.rs          # Spawns QEMU with -smp 4 and disk devices
├── resources/               # Static assets & logos (not deleted by cargo clean)
│   └── img/
│       └── lukes_os_logo.png
├── CHANGELOG.md             # Development milestones & progress tracking
├── Cargo.toml               # Workspace configuration
└── rust-toolchain.toml      # Nightly toolchain specification
```

---

## Getting Started

### Prerequisites

1. **Rust Nightly**:
   ```sh
   rustup toolchain install nightly
   rustup default nightly
   rustup component add rust-src llvm-tools-preview
   rustup target add x86_64-unknown-none
   ```

2. **QEMU**:
   - **Windows**: Install via `winget install SoftwareFreedomConservancy.QEMU` or download from [qemu.org](https://www.qemu.org/download/#windows).
   - **Linux**: `sudo apt install qemu-system-x86`
   - **macOS**: `brew install qemu`

---

### Building and Running

#### 1. One-Step Run (Recommended)

To build the SMP kernel, package boot images, and launch QEMU with 4 CPU cores:

```sh
cargo run
```

#### 2. Manual Kernel Compilation

To compile the bare-metal kernel:

```sh
cargo build --target x86_64-unknown-none -p kernel
```

To run a fast workspace type check:

```sh
cargo check
```

---

## Progress Tracking

For full milestone details, architectural refactors, and future roadmap phases, see:
👉 **[CHANGELOG.md](CHANGELOG.md)**

---

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
