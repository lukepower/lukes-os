# Phase 5 Implementation Plan — Graphical User Interface

> **Audience:** a coding agent working in this repository.
> **Goal:** give Luke's OS a windowed graphical interface. First it runs inside the kernel; later it runs as user-space programs talking to the kernel through system calls.
> **Baseline:** commit `f42c15d` (Phases 0–4 complete per `CHANGELOG.md`).

---

## 0. Ground rules for the agent

1. **Start from a clean tree.** The working tree may contain uncommitted edits. Run `git status`. If anything is modified, stop and ask the user whether to commit or stash it. Never discard it.
2. **One milestone = one or more focused commits.** Use Conventional Commit messages as in the existing history (`feat(kernel): …`, `fix(syscall): …`). After every milestone the OS must still boot to the shell in QEMU.
3. **Build & run:** `cargo run` from the workspace root (default member `runner`). It builds BIOS/UEFI images and launches QEMU with `-serial stdio -m 256M -smp 4` and a VirtIO disk (`disk.img`). The host is Windows with an MSVC toolchain configured in `.cargo/config.toml`, so don't change linker/env settings.
4. **Verify with serial output.** Every new subsystem logs `[OK] …` / `[WARN] …` through `serial_println!`, as `main.rs` already does. Acceptance checks below are written as observable serial or screen results.
5. **Keep it `no_std`.** Kernel target is `x86_64-unknown-none` with `build-std = core, compiler_builtins, alloc`. New crates must work in `no_std` (disable default features where needed).
6. **Document as you go.** For each milestone, append a dated section to `CHANGELOG.md` in the existing style, tick the roadmap item, and update the architecture section of `README.md` when a subsystem is added.
7. **Don't refactor unrelated code.** If you find a bug outside the current milestone, note it in the changelog under "Known issues" instead of fixing it silently.

---

## 1. Current state (relevant facts)

| Area | Where | State |
|---|---|---|
| Framebuffer console | `kernel/src/vga.rs` | Draws 8×16 glyphs straight into the bootloader framebuffer (`FB_INFO: Mutex<Option<FbInfo>>`, BGR, `write_volatile` per byte). `scroll_up` reads back from framebuffer memory, which is slow. |
| Interrupts | `kernel/src/interrupts.rs` | IDT with breakpoint, double fault (IST), page fault, LAPIC timer (`scheduler::timer_interrupt_asm`), reschedule IPI, PS/2 keyboard via 8259 PIC (IRQ1, vector 33). PIC IRQ0 masked. **No mouse (IRQ12).** |
| Keyboard | `kernel/src/keyboard.rs` | `pc-keyboard` decoding into a 256-char ring buffer; `read_char()` blocks by yielding. |
| Scheduler | `kernel/src/scheduler.rs`, `thread.rs` | Per-CPU run queues with work stealing. Context is the GPR set plus the interrupt frame, saved on the kernel stack. **No CR3 switch, no FPU/SSE state.** |
| Memory | `memory.rs`, `allocator.rs` | O(1) frame allocator with `allocate_contiguous`. 4 MiB kernel heap at `0x4444_4444_0000`, growable via `extend_heap`. Physical memory mapped at `PHYS_MEM_OFFSET`. |
| User mode | `gdt.rs`, `syscall.rs`, `elf.rs` | Ring-3 segments, `SYSCALL/SYSRET`, ELF64 loader, syscalls 0–7 (`yield, exit, write, read, open, close, getpid, uname`). |
| SMP | `smp.rs` | Up to 8 cores, `PerCpu` via `IA32_GS_BASE` (GS:[8] = kernel syscall stack, GS:[16] = user RSP scratch). |

### Known defects that block a user-space GUI (verify each before fixing)

- **D1 – GS base handling.** Only `IA32_GS_BASE` (0xC0000101) is written. `IA32_KERNEL_GS_BASE` (0xC0000102) is never set and no `swapgs` happens on entry to ring 3. `syscall_entry` does `swapgs` first, so after it GS base is probably 0 and `GS:[8]` faults. *Check:* run a real ring-3 ELF that calls `SYS_WRITE`. The in-kernel `syscall-test` does not exercise this path.
- **D2 – `exec` hijacks the calling thread.** `elf::spawn_user_process` ends in `enter_user_mode(..) -> !` on the shell thread. The shell never comes back, and the user program isn't a scheduler-managed thread of its own.
- **D3 – Single shared address space.** `spawn_user_process` maps segments into the active kernel page table (`memory::init`). The user stack is always at `0x7FFF_FFFF_0000`. `map_to` errors are silently ignored (`if let Ok(map) = …`), so a second program overlaps the first.
- **D4 – Unchecked user pointers.** `SYS_WRITE` and `SYS_OPEN` call `slice::from_raw_parts` on user-supplied pointers without range or mapping checks.
- **D5 – FPU/SSE.** The AP trampoline doesn't set `CR4.OSFXSR/OSXMMEXCPT`, and context switches don't save XMM state. This is harmless while all code is built for `x86_64-unknown-none` (soft-float, no SSE). It becomes a blocker as soon as any user program is compiled with SSE enabled (needed for Slint or fast float rendering).
- **D6 – Interrupts from ring 3.** `timer_interrupt_asm` and `keyboard_interrupt_handler` use `PerCpu::current()` (GS) and do no `swapgs`. Once D1 is fixed properly (user GS ≠ kernel GS), every interrupt entry that can come from ring 3 must `swapgs` if `CS & 3 == 3`.

---

## 2. Milestones

### M0 — Correctness fixes for user mode (prerequisite for M2+)

Small, high-value fixes. Do them first so later work stands on solid ground.

1. **GS handling (D1, D6)**
   - In `smp::init_bsp` and the AP init path, write `IA32_KERNEL_GS_BASE = 0` and `IA32_GS_BASE = &PerCpu` (kernel convention: while in kernel, GS_BASE = PerCpu).
   - `enter_user_mode` / `user_jump_trampoline`: execute `swapgs` immediately before `iretq`.
   - `syscall_entry`: keep `swapgs` on entry and before `sysretq` (already present). Check that the offsets match `PerCpu` (`#[repr(C)]`, fields at 8 and 16).
   - `timer_interrupt_asm` (and any naked IRQ stub): if the saved CS has RPL 3 (`test qword ptr [rsp+8], 3` before pushing GPRs), `swapgs` on entry and again before `iretq`. For `x86-interrupt` handlers (keyboard, later mouse), either convert them to naked stubs using the same pattern or make sure they never touch GS. A shared macro for the stubs is preferred.
2. **Fail loudly in the ELF loader (D3, partial)**
   - Replace `if let Ok(map) = mapper.map_to(..)` with proper error propagation (`ElfError::MapFailed(page)`).
   - Reject segments with `p_vaddr` outside `0x0000_0000_0040_0000 .. 0x0000_7FFF_0000_0000`.
3. **User pointer validation (D4)**
   - Add `syscall::user_slice(ptr, len) -> Result<&[u8], Errno>` and `user_slice_mut`. Check `ptr + len` doesn't overflow, stays below `0x0000_8000_0000_0000`, and every page is mapped with `USER_ACCESSIBLE` (walk with `mapper.translate`).
   - Use it in every syscall that takes a pointer. Return a negative errno (define `EFAULT = -14`, `EINVAL = -22`, `ENOENT = -2`, `ENOSYS = -38`) instead of panicking.
4. **`exec` spawns instead of hijacking (D2)**
   - Add `scheduler::spawn_user(name, entry, user_stack_top)`. It creates a `Thread` whose initial `Context` has `cs = user_cs | 3`, `ss = user_ds | 3`, `rflags = 0x202`, `rip = entry`, `rsp = user_stack_top`, plus its own kernel stack, which must be set as TSS `RSP0` and `PerCpu.kernel_syscall_stack` when the thread is scheduled.
   - Shell `exec` returns immediately and prints the new TID. `ps` lists it.

**Acceptance (M0):** a small test ELF that calls `write("hello from ring 3\n")` and `exit` runs from the shell via `exec`. The shell stays usable afterwards. `ps` shows the process while it runs. Passing a kernel address to `SYS_WRITE` returns `EFAULT` without crashing.

> Test binary: see M4.1 for the user-space crate layout. For M0, create just `user/hello` and embed it into the RamFS at boot (`include_bytes!` → `/bin/hello`).

---

### M1 — Graphics in the kernel (quick visible win)

Gets windows on screen without depending on M0. Can be done in parallel with or before M0.

1. **Display abstraction — new module `kernel/src/gfx/`**
   - `gfx/display.rs`: `struct Display { front: *mut u8, pitch, width, height, bpp, format: PixelFormat }`, built from the bootloader `FrameBuffer` (move the setup out of `vga.rs`). Support `PixelFormat::Bgr` and `Rgb`, and fall back to a log warning for anything else.
   - `gfx/backbuffer.rs`: a `width × height` `u32` buffer allocated with `memory::allocate_contiguous` (not the heap: 1280×720×4 ≈ 3.6 MB) and mapped at a fixed kernel virtual range, e.g. `0x4444_8000_0000`. Provide `present(&mut self, dirty: Rect)`, which copies only dirty rows to the front buffer with `copy_nonoverlapping` (no per-byte volatile writes).
   - Damage tracking: `DirtyRegion` that merges rectangles. Simplest acceptable version: a single bounding rect per frame.
2. **embedded-graphics**
   - Add `embedded-graphics = { version = "0.8", default-features = false }` (check that the latest 0.8.x builds `no_std`).
   - `impl DrawTarget for BackBuffer` with `Color = Rgb888`, plus `OriginDimensions`. Override `fill_solid` and `fill_contiguous` with row-wise fast paths.
3. **Port the text console onto the back buffer**
   - Make `vga.rs` render into a `Canvas` trait (`put_pixel`, `fill_rect`, `scroll`) instead of the raw framebuffer. Scrolling becomes a `memmove` inside the back buffer, never a read from video memory.
   - Keep `println!` behaviour identical. This step must not change visible output.
4. **PS/2 mouse driver — `kernel/src/mouse.rs`**
   - Initialise the i8042 auxiliary port: enable aux device (`0xA8`), enable IRQ12 in the controller config byte, send `0xF6` (defaults) and `0xF4` (enable streaming) via `0xD4`, and wait for ACK `0xFA`.
   - Unmask IRQ12 on the slave PIC (and IRQ2 cascade on the master). Add `InterruptIndex::Mouse = PIC_2_OFFSET + 4` (vector 44) and a handler that reads port `0x60` and assembles 3-byte packets. Validate with bit 3 of byte 0 and resync if invalid. Send EOI to both PICs.
   - Optional: IntelliMouse scroll wheel (sample rate sequence 200/100/80, then 4-byte packets).
5. **Unified input events — `kernel/src/input.rs`**
   - `enum InputEvent { Key { ch: Option<char>, code: KeyCode, pressed: bool }, MouseMove { dx: i16, dy: i16 }, MouseButton { button: u8, pressed: bool }, Scroll(i8) }`
   - A lock-free or IRQ-safe ring buffer, same approach as `keyboard.rs`. The keyboard handler pushes both into the existing char buffer (the shell keeps working) and into `input` events.
6. **Window manager (kernel thread `"wm"`) — `kernel/src/gfx/wm.rs`**
   - `struct Window { id, title: String, rect: Rect, z: u32, content: Vec<u32> /* own pixel buffer */, dirty: bool }`.
   - Compositor loop (around 60 Hz using LAPIC ticks; `scheduler::sleep` or block on input): drain input, update the cursor, handle focus (click raises), drag by title bar, close button. Then redraw the desktop background, windows in z-order, and the cursor (software cursor drawn last; restore the pixels underneath). Present the dirty rect.
   - Decorations: title bar with text (`embedded-graphics` `MonoTextStyle` with `FONT_6X10`, or the existing 8×16 font), 1-px border, close box. Keep the colour palette in one `theme.rs`.
   - Taskbar at the bottom: one button per window and a clock from ticks.
7. **Console in a window**
   - The shell's text console renders into a `Window.content` buffer instead of the full screen. Keyboard input goes to the focused window. Only the console window feeds `keyboard::INPUT_BUFFER` while focused.
   - Shell command `gui` starts the WM. `text` returns to full-screen console mode, which is useful for debugging. Decide the boot default with a `const GUI_ON_BOOT: bool`.

**Acceptance (M1):** boot in QEMU. A desktop with a taskbar appears, the mouse cursor follows the host mouse, and a "Terminal" window runs the existing shell. Windows can be dragged and focused by clicking. A second demo window ("About Luke's OS" with the logo or text) can be opened from the shell with `about`. No visible tearing during drag. Serial shows `[OK] PS/2 mouse initialized` and `[OK] Window manager started (WxH)`.

QEMU tip: add `-display sdl` or `gtk` and click into the window to grab the mouse. Optionally add `-usb -device usb-tablet` later; it needs a USB stack, so it's out of scope.

---

### M2 — Real processes (separate address spaces)

1. **`Process` structure — new `kernel/src/process.rs`**
   - `struct Process { pid, name, page_table: PhysFrame /* PML4 */, threads: Vec<ThreadId>, fds: FdTable, regions: Vec<VmRegion> }`. Store a `ProcessId` in each `Thread` (`None` = kernel thread).
2. **Per-process page tables**
   - `memory::new_user_page_table()`: allocate a PML4 frame, zero entries 0..256, and copy entries 256..512 from the kernel PML4. All kernel mappings must live in the upper half. **Verify** that the heap (`0x4444_4444_0000`), `PHYS_MEM_OFFSET`, the kernel image, stacks, the back buffer and the AP trampoline are in the upper half or are otherwise mirrored. If the bootloader places things in the lower half, configure `bootloader_api::BootloaderConfig` mappings (`kernel_base`, `physical_memory`, `dynamic_range_start/end`) to push them into the upper half.
   - The ELF loader takes a `&mut OffsetPageTable` built for the target PML4 (not `Cr3::read()`).
   - On free: walk the lower-half tables and deallocate frames on process exit (`SYS_EXIT` of the last thread).
3. **CR3 switching** in `schedule_tick`: if the next thread's process differs from the current one, write CR3 (kernel threads keep the kernel PML4). Also update TSS `RSP0` and `PerCpu.kernel_syscall_stack` to the next thread's kernel stack.
4. **FPU/SSE state (D5)**
   - On every core (BSP and APs): `CR0.EM = 0, CR0.MP = 1, CR4.OSFXSR = 1, CR4.OSXMMEXCPT = 1`, then `fninit`.
   - Per thread: a 512-byte, 16-aligned `FxState`. Eager save/restore with `fxsave64`/`fxrstor64` in the context switch is simplest. Lazy switching via `CR0.TS` is optional later.
   - The kernel stays soft-float. Only user threads need the state, but saving it for all threads is acceptable.
5. **Syscalls added in this milestone:** `SYS_SPAWN(path_ptr, len) -> pid`, `SYS_WAIT(pid) -> exit_code`, `SYS_SBRK(increment)` or `SYS_MMAP_ANON(len) -> addr` (needed for a user heap), `SYS_SLEEP(ms)`, `SYS_TIME() -> ms since boot`.

**Acceptance (M2):** two copies of a test program that write their PID in a loop run at the same time without corrupting each other. Killing one (or letting it exit) frees its frames (check with `mem`). A test program built with `-C target-feature=+sse2` that does float maths gives correct results while another such program runs in parallel.

---

### M3 — Graphics system calls (kernel compositor, user-space clients)

The compositor stays in the kernel (`wm` thread from M1). User programs get windows through syscalls.

1. **Surfaces = shared memory**
   - `SYS_WIN_CREATE(w, h, title_ptr, title_len) -> win_id` allocates `w*h*4` bytes of frames and maps them **both** into the WM's kernel view and into the calling process at a fresh user virtual address. Returns the id; a second syscall `SYS_WIN_BUFFER(win_id) -> user_addr` (or an out-pointer) gives the address.
   - `SYS_WIN_PRESENT(win_id, x, y, w, h)` marks a damage rect, and the WM recomposes.
   - `SYS_WIN_DESTROY(win_id)`. On process exit, destroy all of its windows automatically.
2. **Events**
   - Per-window event queue in the WM. `SYS_WIN_POLL(win_id, *mut Event) -> 0 | 1` is non-blocking; `SYS_WIN_WAIT(win_id, *mut Event)` blocks the thread via `block_current_thread`/`unblock_thread`.
   - `#[repr(C)] struct Event { kind: u32, a: i32, b: i32, c: u32 }`. Kinds: KeyDown, KeyUp, Char, MouseMove (window-local coordinates), MouseDown, MouseUp, Scroll, Resize, Close, FocusIn, FocusOut.
3. **ABI document:** write `docs/ABI.md` listing every syscall number, its registers (`rax` = number; args in `rdi, rsi, rdx, r10, r8, r9`; return in `rax`), and the error codes. Note that `rcx`/`r11` are clobbered by `SYSCALL`, so the 4th argument goes in `r10`.

**Acceptance (M3):** a user program `/bin/clock` opens a 200×80 window, draws the uptime, updates every second, and closes cleanly when the window's close box is clicked (the process exits).

---

### M4 — User-space SDK and demo apps

1. **Workspace layout**
   ```
   user/
     libluke/      # no_std: syscall wrappers, _start, panic handler, global allocator on SYS_MMAP_ANON, print!/println!
     luke-gui/     # no_std + alloc: Window, Event, DrawTarget impl over the shared surface (embedded-graphics), simple widgets (Label, Button, TextBox)
     apps/hello/   # M0 test binary
     apps/clock/
     apps/paint/   # mouse drawing demo
     apps/files/   # lists /disk via SYS_OPEN/readdir (add SYS_READDIR if needed)
   ```
   - Target `x86_64-unknown-none`, linker script `user/link.ld` with base `0x40_0000`, static PIE disabled (`-C relocation-model=static`), `panic = "abort"`.
   - Keep `user/` out of the main workspace `default-members` so `cargo run` stays fast, or build it from `runner/build.rs` first.
2. **Getting binaries into the OS:** the simplest robust option is for `runner/build.rs` to build the user crates and pass their paths via env vars. The kernel embeds them with `include_bytes!(env!("USER_HELLO_PATH"))` and writes them to RamFS `/bin/*` at boot. Later, add a host tool `tools/mklukefs` that writes them into `disk.img` in LukeFs format.
3. **Shell integration:** `exec /bin/clock` and a "Start" menu in the taskbar listing `/bin`.

**Acceptance (M4):** after boot you can launch `clock`, `paint` and `files` from the Start menu, and they run as separate ring-3 processes with their own windows.

---

### M5 — Optional extensions (only when asked)

- **Slint UI:** implement `slint::platform::Platform` in `luke-gui` (software renderer, `renderer-software`, `libm`, `unsafe-single-threaded`, `compat-1-2`). Build that app with SSE enabled (requires M2.4). Check the licence (GPLv3 / royalty-free / commercial).
- **VirtIO-GPU:** use `virtio_drivers::device::gpu::VirtIOGpu` for resolution changes and a hardware cursor. QEMU flag `-device virtio-gpu-pci`. Keep the bootloader framebuffer as fallback.
- **Move the compositor to user space** (SerenityOS-style): requires IPC (message passing syscall) and a way to share surfaces between two processes.
- **Fonts:** TrueType rendering with `fontdue` (`no_std` + alloc) for proportional text.

---

## 3. Suggested order and effort

| Order | Milestone | Rough size | Depends on |
|---|---|---|---|
| 1 | M1 Graphics in kernel | large | — |
| 2 | M0 User-mode fixes | medium | — |
| 3 | M2 Processes, address spaces, FPU | large | M0 |
| 4 | M3 Graphics syscalls | medium | M1, M2 |
| 5 | M4 SDK + apps | medium | M3 |
| 6 | M5 Extensions | open | M4 |

M1 and M0 are independent. Do M1 first for a visible result, or M0 first if user mode is the priority.

## 4. Definition of done (every milestone)

- `cargo build` has no new warnings in changed files, and `cargo run` boots to the shell (or the GUI) with 1 and 4 CPUs (`-smp 1` / `-smp 4`).
- No `unwrap()`/`expect()` on paths reachable from user input or syscalls.
- `CHANGELOG.md` and `README.md` updated, and `docs/ABI.md` updated for syscall changes.
- Short summary to the user: what was done, how it was tested, and any known issues left.
