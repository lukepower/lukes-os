# Development Progress & Changelog — Luke's OS

This document serves as the persistent progress log and architectural changelog for **Luke's OS**. All major milestones, structural refactors, build system adjustments, and subsystem implementations are recorded here.

---

## Roadmap Overview

- [x] **Phase 0: Repository & Build Environment Hygiene**
  - Git repository cleanup, `.gitignore`, asset relocation, build toolchain fixes, `virtio-drivers` 0.12 compatibility.
- [ ] **Phase 1: Core Kernel Architecture & Stability**
  - Robust bitmap/region frame allocator, contiguous DMA allocation for VirtIO, dynamic heap expansion, thread cleanup/reaping.
- [ ] **Phase 2: Input Subsystem & Interactive Shell**
  - Interrupt-safe ring buffer for keyboard input, kernel stdin abstraction, interactive command shell (`help`, `ps`, `mem`, `ls`, `cat`, `lspci`).
- [ ] **Phase 3: Storage & Filesystem Expansion**
  - VFS path resolution (`vfs::open("/path/to/file")`), persistent filesystem driver (FAT32/custom block FS) on top of VirtIO block device.
- [ ] **Phase 4: User Space & System Calls**
  - User mode transitions (Ring 3), TSS RSP0 switching, system call interface (`syscall`/`sysret`), ELF executable loader.

---

## Detailed Milestone Log

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
- **Compiler Warnings**:
  - Fixed function item to integer pointer cast warning in `kernel/src/thread.rs` by casting through `*const ()`.
  - Removed unused imports and variables across `kernel/src/memory.rs`, `kernel/src/ramfs.rs`, `kernel/src/block.rs`, and `kernel/src/virtio_blk.rs`.
- **Build Verification**:
  - `cargo check`: Succeeded with 0 errors.
  - `cargo build --target x86_64-unknown-none -p kernel`: Succeeded.
  - `cargo build -p runner`: Succeeded, generating `rustos-bios.img` (9.9 MB) and `rustos-uefi.img` (9.5 MB).

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
- **Multitasking Foundation**:
  - Context structure preserving callee-saved registers (`rsp`, `rbp`, `rbx`, `r12`-`r15`, `rflags`, `rip`).
  - Assembly context switch routine `switch_context` in `kernel/src/thread.rs`.
  - Round-robin scheduler queue in `kernel/src/scheduler.rs`.
- **Filesystem & Storage Prototype**:
  - Unified `BlockDevice` trait (`kernel/src/block.rs`).
  - PCI bus 0 scanner (`kernel/src/pci.rs`).
  - VirtIO Block Device driver initialization using `virtio-drivers` crate (`kernel/src/virtio_blk.rs`).
  - Virtual File System (`Inode`, `FileSystem`) traits (`kernel/src/vfs.rs`).
  - In-memory filesystem `RamFs` mounted at root (`/`) with support for files, directories, read, and write (`kernel/src/ramfs.rs`).
