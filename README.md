# Luke's OS

![Luke's OS Logo](file:///C:/Users/interski/.gemini/antigravity/brain/e5a081e6-9f4d-4861-afc0-8e8616a99f55/lukes_os_logo.png)

Luke's OS is a hobby operating system written in Rust, designed to explore OS development concepts like memory management, multitasking, and driver implementation.

## Features

- **VGA Text Mode**: Basic text output to the screen.
- **Interrupt Handling**: IDT, GDT, and PIC implementation.
- **Memory Management**: Paging and heap allocation.
- **Multitasking**: Cooperative multitasking with a basic scheduler.
- **Filesystem**: In-memory filesystem (RamFS) with VFS abstraction.
- **PCI Enumeration**: Scans and identifies PCI devices.
- **Keyboard Support**: PS/2 keyboard input handling.

## Getting Started

### Prerequisites

You will need the following tools installed:

- **Rust Nightly**: Since we use unstable features.
  ```sh
  rustup toolchain install nightly
  rustup default nightly
  ```
- **QEMU**: To run the OS.
  - Windows: Download from [qemu.org](https://www.qemu.org/download/#windows).
  - Linux: `sudo apt install qemu-system-x86`
  - macOS: `brew install qemu`

### Building and Running

To build and run Luke's OS in QEMU:

```sh
cargo run
```

This command will:
1. Build the kernel and bootloader.
2. Create a bootable disk image.
3. Launch QEMU with the image.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
