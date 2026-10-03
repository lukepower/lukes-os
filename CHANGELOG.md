# Development Progress & Changelog — Luke's OS

This document serves as the persistent progress log and architectural changelog for **Luke's OS**. All major milestones, structural refactors, build system adjustments, and subsystem implementations are recorded here.

---

## Roadmap Overview

- [x] **Phase 0: Repository & Build Environment Hygiene**
  - Git repository cleanup, `.gitignore`, asset relocation, build toolchain fixes, `virtio-drivers` 0.12 compatibility.
- [x] **Phase 1: Core Kernel Architecture, Memory Overhaul & SMP Bringup**
  - High-performance O(1) physical frame allocator, contiguous DMA allocation for VirtIO, dynamic heap expansion, Symmetric Multiprocessing (SMP / ACPI / APIC) bringup, ticket locks, sleeping mutexes, preemptive timer interrupt handler, and work-stealing scheduler.
- [x] **Phase 2: Input Subsystem & Interactive Shell**
  - Interrupt-safe ring buffer for keyboard input, stdin abstraction (`read_char`, `read_line`), backspace & clear screen support in framebuffer console, and fully interactive kernel shell thread (`help`, `clear`, `mem`, `ps`/`threads`, `lspci`, `ls`, `cat`, `touch`, `mkdir`, `echo`, `uname`).
- [x] **Phase 3: Storage & Filesystem Expansion**
  - Extensible Virtual File System (VFS) with global `MountTable`, multi-mount resolution, `FileHandle`, `OpenFlags`, and `SeekFrom`.
  - Persistent block filesystem `LukeFs` (`kernel/src/lukefs.rs`) on top of VirtIO block device mounted at `/disk`.
  - Interactive shell directory navigation (`cd`, `pwd`), file manipulation (`rm`, `write`, `sync`), and relative/absolute path resolution.
- [x] **Phase 4: User Space & System Calls**
  - User mode transitions (Ring 3), User segment descriptors in GDT, TSS RSP0 switching.
  - Fast system call interface via `SYSCALL`/`SYSRET` MSR configuration (`EFER.SCE`, `LSTAR`, `STAR`, `FMASK`).
  - ELF64 executable loader (`kernel/src/elf.rs`), program header mapper (`PT_LOAD`), user stack allocation.
  - Shell commands `exec`/`run` and `syscall-test`.
- [x] **Phase 5: Graphical User Interface & User Space Hardening**
  - M0: User mode correctness fixes (GS base handling, Ring 3 interrupts swapgs, user pointer validation, non-hijacking process spawning).
  - M1: Display and BackBuffer abstractions, `embedded-graphics` DrawTarget, PS/2 mouse driver (IRQ12), unified input event system, kernel window compositor ("wm"), draggable windows, taskbar, software cursor, and in-window terminal shell.
  - M2: Per-process page tables (PML4 lower-half isolation), preemption CR3 switching, FPU/SSE state management (fxsave64/fxrstor64), and extended process syscalls (`sys_spawn`, `sys_wait`, `sys_sleep`, `sys_time`, `sys_mmap_anon`).

---

## Detailed Milestone Log

### [2026-10-03] Phase 5: Milestone M2 — Real Processes & Address Space Isolation

#### 1. Per-Process Page Tables & Lower-Half Isolation (`memory.rs`, `process.rs`)
- **Process Table & Registry (`kernel/src/process.rs`)**:
  - Implemented `Process` struct tracking `pid`, `name`, `pml4`, threads, and exit codes.
  - Global `PROCESS_TABLE` with `register_process`, `find_process`, and `remove_process`.
- **Per-Process PML4 Tables (`kernel/src/memory.rs`)**:
  - `new_user_page_table(frame_allocator)`: allocates a dedicated PML4 frame, zeros lower half (`0..256`), and mirrors upper half (`256..512`) kernel space.
  - `page_table_for_frame(frame, offset)`: instantiates an `OffsetPageTable` for arbitrary PML4 frames.
  - `free_user_page_table(frame, frame_allocator)`: recursively frees user-mapped page tables and frames upon process exit.
- **Isolated ELF Loading (`kernel/src/elf.rs`)**:
  - `spawn_user_process(path)` loads ELF segments into a freshly allocated per-process PML4.
  - Registers the new `Process` and launches the primary user thread referencing its `ProcessId`.

#### 2. Preemptive CR3 Context Switching (`scheduler.rs`, `thread.rs`)
- Added `process_id` and `fx_state` fields to `Thread`.
- In `schedule_tick`, if the next scheduled thread belongs to a user process, hardware `CR3` is updated to load that process's PML4.

#### 3. FPU & SSE State Management (`fpu.rs`, `scheduler.rs`, `smp.rs`)
- **Hardware Configuration (`kernel/src/fpu.rs`)**:
  - Programmed `CR0` (`EM=0`, `MP=1`) and `CR4` (`OSFXSR=1`, `OSXMMEXCPT=1`) followed by `fninit` on BSP and all secondary AP cores.
- **Thread FPU State Save & Restore (`kernel/src/thread.rs`, `scheduler.rs`)**:
  - Defined 16-byte aligned 512-byte `FxState`.
  - Added eager `fxsave64` and `fxrstor64` assembly routines inside `schedule_tick` to preserve floating-point and SIMD registers across preemption.

#### 4. Extended Process System Calls (`syscall.rs`)
- Implemented `SYS_SPAWN` (8): spawns a new isolated ring-3 ELF process and returns its TID.
- Implemented `SYS_WAIT` (9): blocks until target thread finishes execution.
- Implemented `SYS_SLEEP` (10): sleeps for specified milliseconds via LAPIC tick checks and yielding.
- Implemented `SYS_TIME` (11): returns uptime in milliseconds since boot.
- Implemented `SYS_MMAP_ANON` (12): dynamically allocates contiguous physical frames and maps them into user address space (`0x0000_6000_0000_0000+`) with user-accessible permissions.

---

### [2026-10-03] Phase 5: Milestones M0 & M1 — GUI Engine & User Mode Hardening

#### 1. User Mode Correctness & Process Spawning (M0)
- **GS Base & Interrupt Trampoline Isolation (`smp.rs`, `scheduler.rs`, `elf.rs`)**:
  - Configured `IA32_KERNEL_GS_BASE = 0` on BSP and secondary AP cores.
  - Updated `timer_interrupt_asm` to detect RPL 3 interrupt frames (`test qword ptr [rsp + 8], 3`) and execute `swapgs` on entry and exit.
  - Added `swapgs` prior to `iretq` in `user_jump_trampoline`.
- **ELF Segment Validation & Memory Map Checking (`elf.rs`)**:
  - Enforced valid virtual address bounds for user segments (`0x0040_0000 .. 0x0000_7FFF_0000_0000`), rejecting invalid addresses with `ElfError::InvalidVaddr`.
  - Replaced silent map failures with propagated `ElfError::MapFailed`.
- **User Pointer Validation & System Call Safety (`syscall.rs`)**:
  - Implemented `user_slice(ptr, len)` and `user_slice_mut(ptr, len)` checking address boundaries (< `USER_ADDR_LIMIT`), page accessibility via paging translation, and returning negative POSIX error codes (`EFAULT`, `EINVAL`, `ENOENT`, `ENOSYS`).
- **Scheduler-Managed Process Spawning (`scheduler.rs`, `thread.rs`)**:
  - Implemented `scheduler::spawn_user(name, entry, user_stack_top)` creating a dedicated thread with User Segment selectors and kernel stack.
  - `cmd_exec` spawns the user process and immediately prints the assigned TID without hijacking the interactive shell thread.

#### 2. Display Engine & Double Buffering (M1)
- **Display Abstraction (`gfx/display.rs`)**:
  - Decoupled video memory management from the text console.
  - Added `Display` abstraction over bootloader framebuffer supporting RGB/BGR pixel formats.
- **Physical Contiguous BackBuffer (`gfx/backbuffer.rs`, `gfx/rect.rs`)**:
  - Allocated contiguous physical frames mapped at `0x4444_8000_0000`.
  - Implemented `present()` copying dirty bounding regions row-wise via `copy_nonoverlapping`.
  - Implemented `embedded_graphics::draw_target::DrawTarget` for `BackBuffer` with fast solid and contiguous rectangular fills.

#### 3. PS/2 Mouse Driver & Unified Input System (`mouse.rs`, `input.rs`)
- **PS/2 Auxiliary Device Driver (`mouse.rs`, `interrupts.rs`)**:
  - Programmed i8042 controller (aux enable `0xA8`, streaming `0xF4`, defaults `0xF6`).
  - Unmasked IRQ12 on slave PIC and IRQ2 cascade on master PIC.
  - Assembled 3-byte packets, verified sign/overflow bits, and dispatched normalized mouse motion and button events.
- **Unified Input Queue (`input.rs`)**:
  - Decoupled input queue buffering `InputEvent` records for keyboard and mouse.

#### 4. Kernel Window Compositor & Terminal Integration (`gfx/wm.rs`, `vga.rs`, `shell.rs`)
- **Compositor Thread (`"wm"`)**:
  - Z-ordered window rendering, active/inactive title bars, close buttons, and outer borders.
  - Interactive mouse click-to-focus and title-bar dragging.
  - Bottom taskbar featuring "Luke's OS" menu button, window tabs, and live system uptime clock.
  - 12×18 software mouse cursor pointer rendered on top.
- **Console in a Window & Shell Commands**:
  - Terminal console renders directly into the active Terminal window content buffer in GUI mode.
  - Added shell commands `gui` (activate graphical desktop), `text` (return to full-screen console), and `about` (display Luke's OS dialog window).


### [2026-10-03] Phase 4: User Space Isolation, System Calls & ELF Loader

#### 1. Segment Descriptors & TSS RSP0 (`kernel/src/gdt.rs`)
- **User Mode Segments**:
  - Added User Data Segment (DPL 3, selector index 3) and User Code Segment (DPL 3, selector index 4) to every CPU core's GDT.
  - Exported `selectors(core_id)` returning `user_code_selector` and `user_data_selector`.
- **Privilege Stack Switching (`RSP0`)**:
  - Implemented `set_rsp0(core_id, rsp0)` modifying `TSS.privilege_stack_table[0]` dynamically.
  - Configured each core's dedicated kernel stack so hardware automatically loads a secure kernel stack upon interrupts or privilege escalation from Ring 3.

#### 2. Fast System Call Architecture (`kernel/src/syscall.rs`, `kernel/src/smp.rs`)
- **MSR Hardware Enablement**:
  - Configured `IA32_EFER.SCE` (System Call Extensions).
  - Programmed `IA32_STAR` with kernel and user segment bases (`0x08` for kernel, `0x13` for user).
  - Programmed `IA32_LSTAR` with the 64-bit address of `syscall_entry`.
  - Programmed `IA32_FMASK` to automatically mask `IF` (interrupts), `TF` (trap), and `DF` (direction) flags on entry.
- **Naked Assembly Trampoline (`syscall_entry`)**:
  - Performs `swapgs` to access per-CPU state.
  - Saves user `RSP` in `PerCpu.user_rsp_scratch` (`GS:[16]`) and loads `PerCpu.kernel_syscall_stack` (`GS:[8]`).
  - Pushes user context (`RSP`, `RFLAGS`, `RIP`, callee-saved registers) and invokes `syscall_dispatcher`.
  - Restores user registers, executes `swapgs`, and executes `sysretq` to return cleanly to Ring 3.
- **Dispatcher Implementation**:
  - `SYS_YIELD` (0): Cooperatively yields CPU via `scheduler::yield_now()`.
  - `SYS_EXIT` (1): Terminates calling thread via `scheduler::exit_current_thread()`.
  - `SYS_WRITE` (2): Writes buffer to stdout/stderr or serial log.
  - `SYS_READ` (3): Reads character from stdin via `keyboard::read_char()`.
  - `SYS_OPEN` (4): Opens or creates VFS files by path.
  - `SYS_GETPID` (6): Returns thread ID.
  - `SYS_UNAME` (7): Returns OS version information.

#### 3. ELF64 Executable Loader & Ring 3 Execution (`kernel/src/elf.rs`)
- **ELF64 Parser**:
  - Validates `ELF_MAGIC`, 64-bit architecture (`ELF_CLASS_64`), little-endian format, and executable types (`ET_EXEC` / `ET_DYN`).
  - Parses Program Headers and maps `PT_LOAD` segments to physical frames with appropriate permissions (`USER_ACCESSIBLE`, `WRITABLE`, `NO_EXECUTE`).
  - Allocates and maps an isolated 32 KiB user stack at `0x0000_7FFF_FFFF_0000`.
- **Ring 3 Drop Mechanism (`enter_user_mode`)**:
  - Configures `RSP0` in the TSS.
  - Executes `user_jump_trampoline` which constructs an `iretq` frame (`SS=0x1B`, `RSP=user_stack_top`, `RFLAGS=0x202`, `CS=0x23`, `RIP=entry_point`) and transitions processor to CPL 3.
  - Added high-level `spawn_user_process(path)` to execute ELF binaries stored on `RamFs` or `LukeFs`.

#### 4. Interactive Shell Enhancements (`kernel/src/shell.rs`)
- Added `exec <path>` / `run <path>`: Loads and executes ELF binaries in Ring 3.
- Added `syscall-test`: Tests inline `syscall` instruction dispatch and verifies kernel execution and return value in RAX.

---

### [2026-10-03] Phase 3: Storage, Extensible VFS Architecture & Persistent LukeFs

#### 1. Extensible VFS Core (`kernel/src/vfs.rs`)
- **Mount Point Registry (`MountTable`)**: Replaced single-root filesystem static with a multi-mount table supporting root (`/`) and arbitrary mount points (e.g. `/disk`, `/mnt`).
- **Path Resolution & Normalization**:
  - `normalize_path(path)`: Resolves `.`, `..`, and redundant slashes for canonical path representations.
  - `resolve_path(path)`: Longest-prefix match across mount points traversing to child filesystem root inodes.
- **FileHandle & Stdio-like Operations**:
  - `OpenFlags`: Configurable access flags (`read`, `write`, `create`, `truncate`, `append`).
  - `FileHandle`: Automatic cursor offset tracking with `read()`, `write()`, `seek(SeekFrom)`, and `sync()`.
  - High-level VFS functions: `vfs::open()`, `vfs::mkdir()`, `vfs::unlink()`, `vfs::mount()`, `vfs::unmount()`.
- **Extended Inode Trait**: Added `truncate()`, `unlink()`, and `sync()` to `Inode` trait with default implementations.

#### 2. Persistent Block Filesystem `LukeFs` (`kernel/src/lukefs.rs`)
- **On-Disk Layout**:
  - Sector 0: Superblock magic (`b"LUKEFS01"`).
  - Sectors 1..8: File allocation table storing up to 126 file records (name, `is_dir`, `size`, `start_sector`, `sector_count`).
  - Sector 9+: Data cluster sectors on the underlying `BlockDevice`.
- **Auto-Formatting & Persistence**:
  - `LukeFs::mount(device)`: Checks for superblock signature; auto-formats new disks or reads existing allocation tables into memory.
  - Sector allocation & relocation engine: dynamically allocates and grows sector spans on write.
  - Implements `vfs::FileSystem` and `vfs::Inode` with full read, write, truncate, create, mkdir, unlink, and readdir capabilities.
- **VirtIO Integration**: Mounted persistently at `/disk` using `VirtIoBlockDevice` on `disk.img`.

#### 3. Enhanced Interactive Shell (`kernel/src/shell.rs`)
- **Working Directory Tracking (`cwd`)**: Prompt displays current path (`luke-os:/disk> `).
- **Navigation & Path Operations**:
  - `pwd`: Prints current working directory.
  - `cd <path>`: Changes working directory with validation.
  - `ls [path]`: Lists files and directories relative to `cwd` or by absolute path.
  - `cat <path>`: Displays contents of files in `RamFs` or `LukeFs`.
  - `touch <path>`: Creates empty files at target path.
  - `mkdir <path>`: Creates directories at target path.
  - `rm <path>`: Unlinks and deletes files or directories.
  - `write <path> <text...>`: Writes text directly to persistent or ramfs files.
  - `sync`: Flushes dirty file buffers and metadata to disk.

---

### [2026-10-03] Phase 2: Input Subsystem & Interactive Kernel Shell

#### 1. Interrupt-Safe Keyboard Ring Buffer (`kernel/src/keyboard.rs`)
- **Decoupled Interrupt Handling**: Modified `keyboard_interrupt_handler` in `kernel/src/interrupts.rs` to stop printing raw decoded characters directly to serial/screen. Scancodes from PS/2 port `0x60` are decoded via `pc-keyboard` and pushed into an interrupt-safe FIFO ring buffer (`INPUT_BUFFER`).
- **Input Queuing & Non-blocking/Blocking APIs**:
  - `push_char(ch: char)`: Pushes a character into the 256-character FIFO buffer, dropping the oldest unread character on saturation.
  - `pop_char() -> Option<char>`: Non-blocking character fetch.
  - `read_char() -> char`: Blocking character fetch that cooperatively yields execution (`scheduler::yield_now()`) when the input buffer is empty.
  - `read_line() -> String`: Line buffer with interactive echo to stdout, handling carriage return (`\r`), newline (`\n`), and backspace (`\x08`).

#### 2. Framebuffer Console Enhancements (`kernel/src/vga.rs`)
- **Backspace Support**: Added handling for `\x08` in `Writer::write_char()`, moving the cursor backward (with line wrapping) and clearing character pixel glyphs with background color via `clear_char()`.
- **Screen Clearing**: Added `clear_screen()` to clear the complete framebuffer buffer to black and reset the cursor position to `(0, 0)`.

#### 3. Interactive Kernel Shell (`kernel/src/shell.rs`)
- **Dedicated Shell Thread**: Spawned `"shell"` as a managed kernel thread running `shell::shell_main()`.
- **Builtin Commands**:
  - `help`: Lists all supported shell commands.
  - `clear`: Clears the framebuffer console.
  - `mem`: Inspects physical frame allocator statistics (total, used, free memory) and kernel heap metrics (`allocator::heap_size()`, `allocator::heap_used()`, `allocator::heap_free()`).
  - `ps` / `threads`: Queries the multi-core work-stealing scheduler (`scheduler::list_threads()`) displaying TID, thread name, state, and assigned CPU core.
  - `lspci`: Enumerates all PCI devices discovered on the PCI bus with vendor and device IDs.
  - `ls [path]`: Lists files and directories in the VFS with `[DIR]` and `[FILE]` type badges.
  - `cat <path>`: Reads and prints text files from VFS nodes.
  - `touch <name>`: Creates a file in the root directory.
  - `mkdir <name>`: Creates a directory in the root directory.
  - `echo [text]`: Echoes parameters back to console.
  - `uname`: Displays operating system version and kernel architecture.

---

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
- **ACPI Discovery (`kernel/src/acpi.rs`)**: Parses RSDP, RSDT/XSDT, and the Multiple APIC Description Table (MADT) to discover the Local APIC MMIO address and enumerate secondary processor cores. Utilizes `core::ptr::read_unaligned` to safely parse ACPI tables situated at unaligned BIOS physical addresses.
- **Local APIC (`kernel/src/apic.rs`)**: MSR-based LAPIC enablement, timer configuration (vector `0x20`), EOI signaling, and Inter-Processor Interrupt (IPI) dispatch (INIT, SIPI, reschedule vector `0xEC`).
- **Per-Core GDT & TSS (`kernel/src/gdt.rs`)**: Overcame the 8-slot capacity limit of `x86_64::structures::gdt::GlobalDescriptorTable` by allocating an independent `CPU_GDTS` array of descriptor tables (Code, Data, TSS) and dedicated Double Fault Interrupt Stack Tables for up to 8 CPU cores.
- **Application Processor (AP) Bootloader (`kernel/src/smp.rs`)**:
  - Identity-maps a low-memory 16-bit real-mode to 64-bit long-mode trampoline page at physical address `0x8000`.
  - Dispatches INIT-SIPI-SIPI sequences to wake APs into long mode.
  - Sets up per-core `PerCpu` structures and configures `IA32_GS_BASE` for thread-local core state.
- **Release-Optimized Boot Packaging (`runner/build.rs`)**: Prioritizes release kernel builds (202 KiB) over unoptimized debug kernels (8.8 MiB), reducing BIOS disk loading time from >15 seconds down to under 100ms.
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
