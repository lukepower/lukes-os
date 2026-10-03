# Luke's OS System Call Application Binary Interface (ABI)

## Calling Convention (x86_64 Fast Syscall)

Luke's OS uses hardware-accelerated MSR-based `syscall`/`sysretq` instructions.

- **Syscall Instruction:** `syscall`
- **Syscall Number Register:** `RAX`
- **Arguments:**
  - Argument 1: `RDI`
  - Argument 2: `RSI`
  - Argument 3: `RDX`
  - Argument 4: `R10` (Note: `RCX` is clobbered by `syscall` saving user `RIP`)
  - Argument 5: `R8`
  - Argument 6: `R9`
- **Return Value:** `RAX` (Non-negative indicates success; negative value indicates error code `-errno`)
- **Clobbered Registers:** `RCX` (user `RIP`), `R11` (user `RFLAGS`)

---

## Standard Error Codes

| Name | Value | Description |
|---|---|---|
| `ENOENT` | `-2` | No such file or directory |
| `EFAULT` | `-14` | Bad address (unmapped or inaccessible pointer) |
| `EINVAL` | `-22` | Invalid argument |
| `ENOSYS` | `-38` | Function not implemented |

---

## System Calls

### Core Process & Thread Management

#### 0: `sys_yield`
- **Signature:** `sys_yield() -> 0`
- **Description:** Cooperatively relinquishes remainder of time slice to another ready thread.

#### 1: `sys_exit`
- **Signature:** `sys_exit(exit_code: u64) -> !`
- **Description:** Terminates calling thread and frees resources.

#### 6: `sys_getpid`
- **Signature:** `sys_getpid() -> i64`
- **Description:** Returns the thread/process ID of the calling task.

#### 8: `sys_spawn`
- **Signature:** `sys_spawn(path_ptr: *const u8, path_len: usize) -> i64 (PID / error)`
- **Description:** Loads executable ELF64 binary from VFS path and spawns new thread.

#### 10: `sys_sleep`
- **Signature:** `sys_sleep(ms: u64) -> 0`
- **Description:** Suspends calling thread for specified duration.

#### 11: `sys_time`
- **Signature:** `sys_time() -> i64`
- **Description:** Returns elapsed milliseconds since kernel boot.

---

### File & Stream I/O

#### 2: `sys_write`
- **Signature:** `sys_write(fd: u64, buf_ptr: *const u8, len: usize) -> i64 (bytes written / error)`
- **Description:** Writes buffer to file descriptor (1 = stdout, 2 = stderr).

#### 3: `sys_read`
- **Signature:** `sys_read(fd: u64, buf_ptr: *mut u8, len: usize) -> i64 (bytes read / error)`
- **Description:** Reads up to `len` bytes from file descriptor (0 = stdin).

#### 4: `sys_open`
- **Signature:** `sys_open(path_ptr: *const u8, path_len: usize, write_flag: u64) -> i64 (fd / error)`
- **Description:** Opens file at specified path.

#### 5: `sys_close`
- **Signature:** `sys_close(fd: u64) -> i64`
- **Description:** Closes an open file descriptor.

---

### Graphics & Window Management

#### 20: `sys_win_create`
- **Signature:** `sys_win_create(width: u32, height: u32, title_ptr: *const u8, title_len: usize) -> i64 (win_id / error)`
- **Description:** Requests kernel window manager to allocate and present a managed window.

#### 21: `sys_win_buffer`
- **Signature:** `sys_win_buffer(win_id: u32) -> i64 (user virtual pointer / error)`
- **Description:** Returns user-accessible pointer to 32-bpp pixel buffer.

#### 22: `sys_win_present`
- **Signature:** `sys_win_present(win_id: u32, x: u32, y: u32, w: u32, h: u32) -> i64`
- **Description:** Marks damage rectangle and requests compositor frame flush.

#### 23: `sys_win_poll`
- **Signature:** `sys_win_poll(win_id: u32, event_ptr: *mut Event) -> i64 (1 if event read, 0 if empty)`
- **Description:** Polls next input event for window.

#### 24: `sys_win_destroy`
- **Signature:** `sys_win_destroy(win_id: u32) -> i64`
- **Description:** Destroys window and unmaps surface.

---

### Window Event Structure (`Event`)

```rust
#[repr(C)]
pub struct Event {
    pub kind: u32,
    pub a: i32,
    pub b: i32,
    pub c: u32,
}
```

- `KEY_DOWN = 1`: `a = keycode`, `b = ch as i32`, `c = 1`
- `KEY_UP = 2`: `a = keycode`, `b = ch as i32`, `c = 0`
- `CHAR = 3`: `a = ch as i32`
- `MOUSE_MOVE = 4`: `a = local_x`, `b = local_y`
- `MOUSE_DOWN = 5`: `a = local_x`, `b = local_y`
- `MOUSE_UP = 6`: `a = local_x`, `b = local_y`
- `SCROLL = 7`: `a = delta`
- `RESIZE = 8`: `a = new_width`, `b = new_height`
- `CLOSE = 9`: Window close requested
- `FOCUS_IN = 10`: Window focused
- `FOCUS_OUT = 11`: Window lost focus

