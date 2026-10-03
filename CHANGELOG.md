# Development Progress & Changelog — Luke's OS

This document serves as the persistent progress log and architectural changelog for **Luke's OS**. All major milestones, structural refactors, build system adjustments, and subsystem implementations are recorded here.

---

## Roadmap Overview

- [x] **Phase 0: Repository & Build Environment Hygiene**
  - Git repository cleanup, `.gitignore`, asset relocation, build toolchain fixes, `virtio-drivers` 0.12 compatibility.
- [x] **Phase 1: Core Kernel Architecture, Memory Overhaul & SMP Bringup**
  - High-performance O(1) physical frame allocator, contiguous DMA allocation for VirtIO, dynamic heap expansion, Symmetric Multiprocessing (SMP / ACPI / APIC) bringup, ticket locks, sleeping mutexes, preemptive timer interrupt handler, and work-stealing scheduler.
- [ ] **Phase 2: Input Subsystem & Interactive Shell**
  - Interrupt-safe ring buffer for keyboard input, kernel stdin abstraction, interactive command shell (`help`, `ps`, `mem`, `ls`, `cat`, `lspci`).
- [ ] **Phase 3: Storage & Filesystem Expansion**
  - VFS path resolution (`vfs::open("/path/to/file")`), persistent filesystem driver (FAT32/custom block FS) on top of VirtIO block device.
- [ ] **Phase 4: User Space & System Calls**
  - User mode transitions (Ring 3), TSS RSP0 switching, system call interface (`syscall`/`sysret`), ELF executable loader.

---

## Detailed Milestone Log

### [2026-10-03] Phase 1: Core Architecture, Memory Allocators & SMP Bringup

#### 1. Physical Frame Allocator Overhaul (`kernel/src/memory.rs`)
- **O(1) Allocation & Deallocation**: Replaced the inefficient $O(N)$ linear scan with an intrusive free-list and multi-region bump allocator. Freed frames store the next pointer directly within their physical page via `PHYS_MEM_OFFSET`.
- **Contiguous DMA Allocations**: Implemented `allocate_contiguous(pages)` and `deallocate_contiguous(frame, pages)` to support multi-page contiguous physical allocations needed by hardware drivers.
- **Diagnostics**: Added real-time tracking for total memory, allocated frames, and free memory (`total_memory_bytes`, `free_memory_bytes`).
- **VirtIO HAL Integration (`kernel/src/virtio_hal.rs`)**:
  - `VirtioHal::dma_alloc` now safely calls `allocate_contiguous`, eliminating multi-page non-contiguous panics.
  - `VirtioHal::dma_dealloc` now reclaims DMA memory via `deallocate_contiguous`, eliminating memory leaks during disk I/O.

#### 2. Dynamic Kernel Heap Expansion (`kernel/src/allocator.rs`)
- **Expanded Capacity**: Increased initial heap size from 256 KiB to **4 MiB** (`0x4444_4444_0000`).
- **Dynamic Growth**: Implemented `extend_heap(mapper, frame_allocator, additional_bytes)` to map additional physical frames and expand the heap boundary dynamically at runtime.
- **Heap Diagnostics**: Added `heap_size()`, `heap_used()`, and `heap_free()` queries.

#### 3. Symmetric Multiprocessing (SMP) Bringup
- **ACPI Discovery (`kernel/src/acpi.rs`)**: Parses RSDP, RSDT/XSDT, and the Multiple APIC Description Table (MADT) to discover the Local APIC MMIO address and enumerate secondary processor cores.
- **Local APIC (`kernel/src/apic.rs`)**: MSR-based LAPIC enablement, timer configuration, EOI signaling, and Inter-Processor Interrupt (IPI) dispatch.
- **Per-Core GDT & TSS (`kernel/src/gdt.rs`)**: Configured per-core descriptor tables and dedicated Double Fault Interrupt Stack Tables for up to 8 CPU cores.
- **Application Processor (AP) Bootloader (`kernel/src/smp.rs`)**:
  - Identity-maps a low-memory 16-bit real-mode to 64-bit long-mode trampoline page at physical address `0x8000`.
  - Dispatches INIT-SIPI-SIPI sequences to wake APs.
  - Sets up per-core `PerCpu` structures and configures `IA32_GS_BASE` for thread-local core state.
- **Runner Configuration (`runner/src/main.rs`)**: Configured QEMU launcher with `-smp 4`.

#### 4. Synchronization Primitives (`kernel/src/sync.rs`)
- **TicketLock**: Fair FIFO spinlock preventing core starvation under high multi-core contention.
- **WaitQueue & SleepingMutex**: Implemented non-busy-waiting mutexes that deschedule and block threads until locks become available.

#### 5. Preemptive Scheduling & Thread Lifecycle (`kernel/src/scheduler.rs`, `kernel/src/thread.rs`)
- **Preemptive Timer ISR**: Implemented `timer_interrupt_asm` which captures full register frames (`r15`..`rax`, `rip`, `cs`, `rflags`, `rsp`, `ss`) and invokes `timer_interrupt_handler` on LAPIC ticks.
- **Safe Zombie Reaping**: Deferring stack deallocation of exiting threads to a zombie queue (`sched.dead`), completely eliminating stack use-after-free bugs.
- **Thread States**: Added `Sleeping` and `Blocked` states, supporting `scheduler::sleep(ticks)`, `block_current_thread()`, and `unblock_thread(id)`.
- **Work-Stealing & Diagnostics**: Implemented core-aware scheduling queues and `list_threads()` for system introspection.

---

### [2026-10-03] Phase 0: Repository Hygiene, Build Pipeline & Driver Fixes

#### 1. Repository Clean-Up & Git Tracking
- **Added `.gitignore`**: Excluded `/target/`, `*.img`, `*.bin`, and local Cargo configs to prevent binary bloat.
- **Untracked Cached Binaries**: Removed over 1,700 incremental compilation objects under `target/` and the 16 MB `disk.img` from git tracking while preserving local development files.
- **Asset Relocation**: Moved project logos from the build output directory `target/resources/img/` to a permanent top-level directory `resources/img/` (`logo.png`, `lukes_os_logo.png`, `lukes_os_logo_1.png`) to prevent accidental deletion during `cargo clean`.
- **Deduplication in Runner**: Removed a duplicate 32 MB dummy disk creation block in `runner/src/main.rs`.

#### 2. Host Build System & MSVC Environment Configuration
- **MSVC Library Resolution**: Configured `[env] LIB` inside `.cargo/config.toml` to automatically resolve Microsoft Visual Studio OneCore libraries and Windows SDK UCRT/UM paths (`msvcrt.lib`, `ucrt.lib`, `kernel32.lib`) when compiling host build scripts on Windows.
- **Panic Profile**: Configured `panic = "abort"` for both `dev` and `release` profiles in the workspace root `Cargo.toml` to satisfy `no_std` kernel requirements under Rust Nightly.

#### 3. Kernel Codebase & API Modernization
- **VirtIO Drivers 0.12 Compliance**: Updated `impl ConfigurationAccess for PciConfig` in `kernel/src/virtio_blk.rs` so that `read_word` and `write_word` match the required safe method signatures while safely encapsulating port I/O.
- **Compiler Warnings**: Fixed function pointer cast warnings and cleaned unused imports.
- **Build Verification**: `cargo check`, kernel build, and runner image builds all verified.

---

### [2026-02-17] Initial Kernel Prototype

- **Bootstrapping**: Configured `bootloader_api` (v0.11) with dynamic physical memory mapping for UEFI & BIOS boots.
- **CPU & Interrupts**:
  - Implemented Global Descriptor Table (GDT) and Task State Segment (TSS) with a dedicated Double Fault IST stack (`kernel/src/gdt.rs`).
  - Configured Interrupt Descriptor Table (IDT) for Breakpoint, Double Fault, Page Fault, Timer (PIT IRQ0), and PS/2 Keyboard (IRQ1) (`kernel/src/interrupts.rs`).
  - Dual 8259 PIC initialization with offsets 32 and 40.
- **Visual & Debug Output**:
  - Framebuffer-based text console with 8x16 font, screen scrolling, and German character support (`kernel/src/vga.rs`).
  - 16550 UART COM1 serial logger (`kernel/src/serial.rs`).
- **Memory Management**:
  - Virtual-to-physical mapper using `x86_64::structures::paging::OffsetPageTable`.
  - `BootInfoFrameAllocator` providing 4 KiB physical frames from bootloader memory regions.
  - 256 KiB fixed heap initialized via `linked_list_allocator::LockedHeap` at virtual address `0x4444_4444_0000`.
- **Filesystem & Storage Prototype**:
  - Unified `BlockDevice` trait (`kernel/src/block.rs`).
  - PCI bus 0 scanner (`kernel/src/pci.rs`).
  - VirtIO Block Device driver initialization using `virtio-drivers` crate (`kernel/src/virtio_blk.rs`).
  - Virtual File System (`Inode`, `FileSystem`) traits (`kernel/src/vfs.rs`).
  - In-memory filesystem `RamFs` mounted at root (`/`) with support for files, directories, read, and write (`kernel/src/ramfs.rs`).
