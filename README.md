# Luke's OS

<p align="center">
  <img src="resources/img/lukes_os_logo.png" alt="Luke's OS Logo" width="480">
</p>

**Luke's OS** is an educational, 64-bit x86 hobby operating system developed in **Rust**. It aims to explore modern OS architecture principles — from bare-metal bootstrapping and virtual memory management to preemptive multitasking, PCI enumeration, VirtIO block storage, and virtual file systems.

---

## Architecture & Subsystems

```
                                +---------------------------+
                                |        Luke's OS          |
                                +---------------------------+
                                              |
        +------------------+------------------+------------------+------------------+
        |                  |                  |                  |                  |
+---------------+  +---------------+  +---------------+  +---------------+  +---------------+
|  CPU & Boot   |  | Memory Engine |  | Multitasking  |  | Console & IO  |  | Storage & VFS |
+---------------+  +---------------+  +---------------+  +---------------+  +---------------+
| • GDT / TSS   |  | • Paging      |  | • Round-Robin |  | • Framebuffer |  | • PCI Scanner |
| • IDT (x86)   |  | • Frame Alloc |  | • Context Sw. |  |   Text Console|  | • VirtIO-Blk  |
| • 8259 PIC    |  | • 256KB Heap  |  | • Yield / HLT |  | • 16550 Serial|  | • VFS / RamFS |
+---------------+  +---------------+  +---------------+  +---------------+  +---------------+
```

### 1. Bootstrapping & Target Architecture
- **Bootloader**: Built on `bootloader_api` (v0.11), supporting both **BIOS** (`rustos-bios.img`) and **UEFI** (`rustos-uefi.img`) boot modes.
- **Bare-Metal Target**: Compiles against `x86_64-unknown-none` using `build-std = ["core", "compiler_builtins", "alloc"]` and `panic = "abort"`.
- **Physical Memory Mapping**: Maps all physical memory at a dynamic virtual offset (`Mapping::Dynamic`) provided by the bootloader.

### 2. CPU Descriptors & Interrupt Handling
- **Global Descriptor Table (GDT)**: Configures kernel code and data segments along with a Task State Segment (TSS).
- **Interrupt Stack Table (IST)**: Dedicated stack for Double Fault exceptions to guarantee diagnostics even during kernel stack overflows.
- **Interrupt Descriptor Table (IDT)**: Handlers for Breakpoint, Double Fault, Page Fault, Timer (IRQ0), and PS/2 Keyboard (IRQ1).
- **Programmable Interrupt Controller (PIC)**: Dual 8259 PIC remapped to hardware vectors 32..47.

### 3. Display & Serial Output
- **Framebuffer Console (`kernel/src/vga.rs`)**: Direct pixel-rendered text output using a custom 8x16 font. Features screen scrolling and support for ASCII as well as extended European characters (`ä`, `ö`, `ü`, `ß`, `€`, `°`, etc.).
- **Serial Logger (`kernel/src/serial.rs`)**: COM1 UART output (`0x3F8`) allowing headless debugging and automated test log capture via `serial_println!`.

### 4. Memory Management
- **Paging**: Recursive 4-level page table management via `x86_64::structures::paging::OffsetPageTable`.
- **Physical Frame Allocation**: Bootloader memory region parser distributing 4 KiB usable frames.
- **Kernel Heap**: Initialized at virtual address `0x4444_4444_0000` using a `LockedHeap` linked-list allocator, providing Rust standard collections (`Vec`, `String`, `Box`, `BTreeMap`).

### 5. Multitasking & Cooperative Scheduler
- **Thread Context**: Captures all callee-saved registers (`rsp`, `rbp`, `rbx`, `r12`-`r15`, `rflags`, `rip`).
- **Context Switcher**: Naked assembly routine (`switch_context`) performing fast register swapping without holding scheduler spinlocks.
- **Scheduler**: Round-robin run queue maintaining active threads with cooperative yielding and tick notification.

### 6. Storage & Virtual File System (VFS)
- **PCI Enumeration**: Configuration-space scanner probing bus 0 for devices and function endpoints.
- **VirtIO Block Driver**: Implements a storage driver on top of `virtio-drivers` 0.12, capable of reading and writing raw disk sectors.
- **VFS Interface**: Storage-agnostic `Inode` and `FileSystem` traits with error abstractions.
- **RamFS**: In-memory file system mounted at `/` supporting hierarchical directories and dynamic file data.

### 7. Input
- **PS/2 Keyboard**: Scancode processing using the `pc-keyboard` crate with German De105Key layout support.

---

## Project Structure

```
lukes-os/
├── .cargo/
│   └── config.toml          # build-std flags, target configuration & MSVC env
├── kernel/                  # Bare-metal x86_64 kernel (no_std)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs          # Kernel entry point and initialization sequence
│       ├── allocator.rs     # Kernel heap initialization (LockedHeap)
│       ├── block.rs         # Unified BlockDevice trait
│       ├── gdt.rs           # GDT, TSS, and IST setup
│       ├── interrupts.rs    # IDT handlers, PIC remapping, timer ticks
│       ├── keyboard.rs      # PS/2 scancode decoder
│       ├── memory.rs        # Page table initialization & frame allocator
│       ├── pci.rs           # PCI bus scanner and port I/O
│       ├── ramfs.rs         # In-memory filesystem implementation
│       ├── scheduler.rs     # Round-robin thread scheduler
│       ├── serial.rs        # COM1 16550 UART driver & macros
│       ├── thread.rs        # Thread state, stack allocation & switch_context
│       ├── vfs.rs           # VFS abstraction traits (Inode, FileSystem)
│       ├── vga.rs           # Framebuffer text console with bitmap font
│       ├── virtio_blk.rs    # VirtIO block device driver
│       └── virtio_hal.rs    # Hardware Abstraction Layer for virtio-drivers
├── runner/                  # Host launcher and disk packaging tool
│   ├── build.rs             # Builds BIOS/UEFI images via bootloader crate
│   ├── Cargo.toml
│   └── src/
│       └── main.rs          # Locates QEMU, creates disk.img & launches VM
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

1. **Rust Nightly**: Required for unstable compiler features (`abi_x86_interrupt`, `build-std`, `naked_asm`).
   ```sh
   rustup toolchain install nightly
   rustup default nightly
   rustup component add rust-src llvm-tools-preview
   rustup target add x86_64-unknown-none
   ```

2. **QEMU**: Used to run and test the operating system.
   - **Windows**: Download installer from [qemu.org](https://www.qemu.org/download/#windows) or install via `winget install SoftwareFreedomConservancy.QEMU`.
   - **Linux**: `sudo apt install qemu-system-x86`
   - **macOS**: `brew install qemu`

---

### Building and Running

#### 1. One-Step Run (Recommended)

To build the kernel, package the bootable disk image, and launch QEMU:

```sh
cargo run
```

This runs the host `runner` binary, which:
1. Verifies that the kernel is compiled.
2. Invokes the `bootloader` crate to generate `target/rustos-bios.img` and `target/rustos-uefi.img`.
3. Creates a dummy `disk.img` for VirtIO if one does not exist.
4. Spawns QEMU with the appropriate virtual hardware, framebuffer, and serial console redirection.

#### 2. Manual Kernel Build & Inspection

To compile only the bare-metal kernel:

```sh
cargo build --target x86_64-unknown-none -p kernel
```

To run a fast workspace type check:

```sh
cargo check
```

---

## Progress Tracking

For detailed historical updates, recent architectural changes, and upcoming roadmap items, refer to:
👉 **[CHANGELOG.md](CHANGELOG.md)**

---

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
