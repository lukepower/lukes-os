use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::{allocator, memory, pci, scheduler, vfs, vga};

/// Entry point for the interactive kernel shell thread.
pub fn shell_main() {
    // Print banner
    crate::println!("\n========================================");
    crate::println!("  Luke's OS Interactive Kernel Shell   ");
    crate::println!("  Type 'help' for available commands.   ");
    crate::println!("========================================\n");

    let mut cwd = String::from("/");

    loop {
        crate::print!("luke-os:{}> ", cwd);
        let line = crate::keyboard::read_line();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        execute_command(trimmed, &mut cwd);
    }
}

fn execute_command(input: &str, cwd: &mut String) {
    let mut parts = input.split_whitespace();
    let command = match parts.next() {
        Some(cmd) => cmd,
        None => return,
    };
    let args: Vec<&str> = parts.collect();

    match command {
        "help" => cmd_help(),
        "clear" => cmd_clear(),
        "mem" => cmd_mem(),
        "ps" | "threads" => cmd_threads(),
        "lspci" => cmd_lspci(),
        "pwd" => cmd_pwd(cwd),
        "cd" => cmd_cd(args.get(0).copied(), cwd),
        "ls" => cmd_ls(args.get(0).copied(), cwd),
        "cat" => cmd_cat(args.get(0).copied(), cwd),
        "touch" => cmd_touch(args.get(0).copied(), cwd),
        "mkdir" => cmd_mkdir(args.get(0).copied(), cwd),
        "rm" => cmd_rm(args.get(0).copied(), cwd),
        "write" => cmd_write(&args, cwd),
        "echo" => cmd_echo(&args),
        "sync" => cmd_sync(),
        "exec" | "run" => cmd_exec(args.get(0).copied(), cwd),
        "syscall-test" => cmd_syscall_test(),
        "uname" => cmd_uname(),
        "gui" => cmd_gui(),
        "text" => cmd_text(),
        "about" => cmd_about(),
        _ => {
            crate::println!("Unknown command: '{}'. Type 'help' for available commands.", command);
        }
    }
}

fn cmd_help() {
    crate::println!("Available commands:");
    crate::println!("  help         - Display this list of commands");
    crate::println!("  clear        - Clear the framebuffer console");
    crate::println!("  mem          - Display physical frame & kernel heap usage");
    crate::println!("  ps / threads - Display active threads across CPU cores");
    crate::println!("  lspci        - Scan and display devices on PCI bus");
    crate::println!("  pwd          - Print current working directory");
    crate::println!("  cd <path>    - Change current working directory");
    crate::println!("  ls [path]    - List directory entries");
    crate::println!("  cat <path>   - Display file contents");
    crate::println!("  touch <path> - Create an empty file");
    crate::println!("  mkdir <path> - Create a directory");
    crate::println!("  rm <path>    - Remove a file or empty directory");
    crate::println!("  write <path> <text...> - Write text to a file");
    crate::println!("  sync         - Flush filesystem metadata to disk");
    crate::println!("  exec <path>  - Load ELF64 binary and transition to Ring 3");
    crate::println!("  syscall-test - Execute userspace syscall demonstration");
    crate::println!("  echo [text]  - Echo text back to console");
    crate::println!("  uname        - Display OS system information");
    crate::println!("  gui          - Switch to Graphical Window Manager desktop");
    crate::println!("  text         - Return to full-screen text console mode");
    crate::println!("  about        - Open Luke's OS About dialog window");
}

fn cmd_clear() {
    vga::clear_screen();
}

fn cmd_mem() {
    crate::println!("--- Physical Memory ---");
    if let Some((total, used, free)) = memory::physical_memory_stats() {
        crate::println!(
            "  Total: {} KiB ({} MiB)",
            total / 1024,
            total / (1024 * 1024)
        );
        crate::println!(
            "  Used:  {} KiB ({} MiB)",
            used / 1024,
            used / (1024 * 1024)
        );
        crate::println!(
            "  Free:  {} KiB ({} MiB)",
            free / 1024,
            free / (1024 * 1024)
        );
    } else {
        crate::println!("  Physical memory stats unavailable");
    }

    crate::println!("--- Kernel Heap ---");
    let heap_size = allocator::heap_size();
    let heap_used = allocator::heap_used();
    let heap_free = allocator::heap_free();
    crate::println!("  Size:  {} bytes ({} KiB)", heap_size, heap_size / 1024);
    crate::println!("  Used:  {} bytes ({} KiB)", heap_used, heap_used / 1024);
    crate::println!("  Free:  {} bytes ({} KiB)", heap_free, heap_free / 1024);
}

fn cmd_threads() {
    crate::println!("{:<6} {:<16} {:<12} {:<8}", "TID", "NAME", "STATE", "CORE");
    crate::println!("{:-<6} {:-<16} {:-<12} {:-<8}", "", "", "", "");
    let threads = scheduler::list_threads();
    for t in threads {
        crate::println!("{:<6} {:<16} {:<12?} Core {}", t.id, t.name, t.state, t.core_id);
    }
}

fn cmd_lspci() {
    let devices = pci::scan_bus();
    crate::println!("Found {} PCI device(s):", devices.len());
    for dev in devices {
        crate::println!(
            "  {:02x}:{:02x}.{} Vendor:{:04x} Device:{:04x} Class:{:02x} Subclass:{:02x}",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            dev.class_id,
            dev.subclass_id
        );
    }
}

fn combine_path(cwd: &str, target: &str) -> String {
    if target.starts_with('/') {
        vfs::normalize_path(target)
    } else {
        let mut combined = String::from(cwd);
        if !combined.ends_with('/') {
            combined.push('/');
        }
        combined.push_str(target);
        vfs::normalize_path(&combined)
    }
}

fn cmd_pwd(cwd: &str) {
    crate::println!("{}", cwd);
}

fn cmd_cd(path: Option<&str>, cwd: &mut String) {
    let target = path.unwrap_or("/");
    let resolved_path = combine_path(cwd, target);

    match vfs::resolve_path(&resolved_path) {
        Ok(inode) => match inode.metadata() {
            Ok(meta) => {
                if meta.file_type == vfs::FileType::Directory {
                    *cwd = resolved_path;
                } else {
                    crate::println!("cd: '{}' is not a directory", target);
                }
            }
            Err(e) => crate::println!("cd error: {:?}", e),
        },
        Err(e) => crate::println!("cd: path not found '{}' ({:?})", target, e),
    }
}

fn cmd_ls(path: Option<&str>, cwd: &str) {
    let target_path = match path {
        Some(p) => combine_path(cwd, p),
        None => cwd.to_string(),
    };

    let node = match vfs::resolve_path(&target_path) {
        Ok(n) => n,
        Err(e) => {
            crate::println!("ls: cannot access '{}': {:?}", target_path, e);
            return;
        }
    };

    match node.readdir() {
        Ok(entries) => {
            if entries.is_empty() {
                crate::println!("(empty directory)");
            } else {
                for name in entries {
                    let type_marker = match node.lookup(&name) {
                        Ok(child) => match child.metadata() {
                            Ok(meta) => match meta.file_type {
                                vfs::FileType::Directory => "[DIR] ",
                                vfs::FileType::File => "[FILE]",
                            },
                            Err(_) => "[?]   ",
                        },
                        Err(_) => "[?]   ",
                    };
                    crate::println!("  {} {}", type_marker, name);
                }
            }
        }
        Err(e) => {
            crate::println!("ls error: {:?}", e);
        }
    }
}

fn cmd_cat(path: Option<&str>, cwd: &str) {
    let path = match path {
        Some(p) => p,
        None => {
            crate::println!("Usage: cat <filename or path>");
            return;
        }
    };

    let full_path = combine_path(cwd, path);
    match vfs::open(&full_path, vfs::OpenFlags::READ) {
        Ok(mut handle) => {
            let meta = match handle.metadata() {
                Ok(m) => m,
                Err(e) => {
                    crate::println!("Failed to read metadata: {:?}", e);
                    return;
                }
            };

            if meta.file_type == vfs::FileType::Directory {
                crate::println!("cat: '{}' is a directory", path);
                return;
            }

            let size = meta.size as usize;
            if size == 0 {
                return;
            }

            let mut buf = alloc::vec![0u8; size];
            match handle.read(&mut buf) {
                Ok(bytes_read) => match core::str::from_utf8(&buf[..bytes_read]) {
                    Ok(content) => crate::println!("{}", content),
                    Err(_) => crate::println!("<binary content: {} bytes>", bytes_read),
                },
                Err(e) => crate::println!("Failed to read file: {:?}", e),
            }
        }
        Err(e) => crate::println!("cat: cannot open '{}': {:?}", full_path, e),
    }
}

fn cmd_touch(name: Option<&str>, cwd: &str) {
    let filename = match name {
        Some(n) if !n.is_empty() => n,
        _ => {
            crate::println!("Usage: touch <filename>");
            return;
        }
    };

    let full_path = combine_path(cwd, filename);
    match vfs::open(&full_path, vfs::OpenFlags::CREATE_OR_TRUNCATE) {
        Ok(_) => crate::println!("Created file '{}'", full_path),
        Err(e) => crate::println!("Failed to create file '{}': {:?}", full_path, e),
    }
}

fn cmd_mkdir(name: Option<&str>, cwd: &str) {
    let dirname = match name {
        Some(n) if !n.is_empty() => n,
        _ => {
            crate::println!("Usage: mkdir <dirname>");
            return;
        }
    };

    let full_path = combine_path(cwd, dirname);
    match vfs::mkdir(&full_path) {
        Ok(_) => crate::println!("Created directory '{}'", full_path),
        Err(e) => crate::println!("Failed to create directory '{}': {:?}", full_path, e),
    }
}

fn cmd_rm(name: Option<&str>, cwd: &str) {
    let target = match name {
        Some(n) if !n.is_empty() => n,
        _ => {
            crate::println!("Usage: rm <filename or path>");
            return;
        }
    };

    let full_path = combine_path(cwd, target);
    match vfs::unlink(&full_path) {
        Ok(_) => crate::println!("Removed '{}'", full_path),
        Err(e) => crate::println!("Failed to remove '{}': {:?}", full_path, e),
    }
}

fn cmd_write(args: &[&str], cwd: &str) {
    if args.is_empty() {
        crate::println!("Usage: write <file> <text...>");
        return;
    }

    let file_path = args[0];
    let full_path = combine_path(cwd, file_path);

    let mut content = String::new();
    for (i, word) in args[1..].iter().enumerate() {
        if i > 0 {
            content.push(' ');
        }
        content.push_str(word);
    }

    match vfs::open(&full_path, vfs::OpenFlags::CREATE_OR_TRUNCATE) {
        Ok(mut handle) => match handle.write(content.as_bytes()) {
            Ok(bytes) => crate::println!("Wrote {} bytes to '{}'", bytes, full_path),
            Err(e) => crate::println!("Failed to write to '{}': {:?}", full_path, e),
        },
        Err(e) => crate::println!("Failed to open '{}': {:?}", full_path, e),
    }
}

fn cmd_sync() {
    if let Some(root_node) = vfs::root() {
        let _ = root_node.sync();
    }
    crate::println!("Filesystem sync complete.");
}

fn cmd_echo(args: &[&str]) {
    for (i, word) in args.iter().enumerate() {
        if i > 0 {
            crate::print!(" ");
        }
        crate::print!("{}", word);
    }
    crate::println!();
}

fn cmd_exec(path: Option<&str>, cwd: &str) {
    let p = match path {
        Some(path) => path,
        None => {
            crate::println!("Usage: exec <path-to-elf>");
            return;
        }
    };

    let full_path = combine_path(cwd, p);
    crate::println!("Loading ELF binary from '{}'...", full_path);
    match crate::elf::spawn_user_process(&full_path) {
        Ok(tid) => {
            crate::println!("Spawned user process with TID={}", tid.0);
        }
        Err(e) => {
            crate::println!("Execution failed: {}", e);
        }
    }
}

fn cmd_syscall_test() {
    crate::println!("Invoking userspace syscall simulation...");
    let message = b"Hello from simulated userspace via SYSCALL instruction!\n";

    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") crate::syscall::SYS_WRITE,
            in("rdi") 1u64,                                   // stdout
            in("rsi") message.as_ptr() as u64,               // buf ptr
            in("rdx") message.len() as u64,                  // length
            lateout("rax") ret,
            out("rcx") _,
            out("r11") _,
        );
    }
    crate::println!("Syscall return value: {}", ret);
}

fn cmd_uname() {
    crate::println!("Luke's OS 0.2.0 x86_64 (SMP preemptive microkernel with Ring 3 & Syscalls)");
}

fn cmd_gui() {
    crate::gfx::wm::GUI_MODE.store(true, core::sync::atomic::Ordering::Relaxed);
    crate::println!("Switched to GUI desktop mode.");
}

fn cmd_text() {
    crate::gfx::wm::GUI_MODE.store(false, core::sync::atomic::Ordering::Relaxed);
    vga::clear_screen();
    crate::println!("Returned to text console mode.");
}

fn cmd_about() {
    let win_id = crate::gfx::wm::create_window("About Luke's OS", 240, 160, 360, 200, true);
    // Draw some text in the about window content buffer
    let mut wm = crate::gfx::wm::WM.lock();
    if let Some(win) = wm.windows.iter_mut().find(|w| w.id == win_id) {
        win.content.fill(0x001E293B); // Slate dark blue
        let w = win.content_width as usize;
        let h = win.content_height as usize;

        let lines = [
            "============================",
            "        Luke's OS           ",
            "     Version 0.3.0 GUI      ",
            "============================",
            "",
            " • 64-bit SMP Multiprocessing",
            " • Preemptive Work-Stealing  ",
            " • Kernel Window Compositor  ",
            " • PS/2 Mouse & Keyboard     ",
            " • Extensible VFS & LukeFs   ",
        ];

        for (line_idx, line) in lines.iter().enumerate() {
            let y0 = 16 + line_idx * 16;
            for (char_idx, ch) in line.chars().enumerate() {
                let glyph = crate::vga::font::glyph(ch);
                let x0 = 16 + char_idx * 8;
                for (dy, &glyph_row) in glyph.iter().enumerate() {
                    let py = y0 + dy;
                    if py >= h {
                        continue;
                    }
                    for dx in 0..8 {
                        let px = x0 + dx;
                        if px >= w {
                            continue;
                        }
                        if (glyph_row >> (7 - dx)) & 1 != 0 {
                            win.content[py * w + px] = 0x00F8FAFC;
                        }
                    }
                }
            }
        }
    }
    crate::println!("Opened About window (id={})", win_id);
}
