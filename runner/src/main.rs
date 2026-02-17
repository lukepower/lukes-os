use std::path::PathBuf;
use std::process::Command;

fn find_qemu() -> String {
    // Check PATH first
    if Command::new("qemu-system-x86_64")
        .arg("--version")
        .output()
        .is_ok()
    {
        return "qemu-system-x86_64".to_string();
    }

    // Common Windows install locations
    let candidates = [
        r"C:\Program Files\QEMU\qemu-system-x86_64.exe",
        r"C:\Program Files (x86)\QEMU\qemu-system-x86_64.exe",
    ];

    for path in &candidates {
        if PathBuf::from(path).exists() {
            return path.to_string();
        }
    }

    // Fall back and let the OS error explain
    "qemu-system-x86_64".to_string()
}

fn main() {
    let bios_image = env!("BIOS_IMAGE");
    let qemu = find_qemu();

    println!("=== Luke's OS QEMU Launcher ===");
    println!("BIOS image: {}", bios_image);
    println!("QEMU binary: {}", qemu);

    // Create a dummy disk image if not exists
    if !std::path::Path::new("disk.img").exists() {
        use std::io::Write;
        let mut file = std::fs::File::create("disk.img").unwrap();
        file.set_len(16 * 1024 * 1024).unwrap(); // 16 MB
        file.write_all(b"Hello VirtIO!").unwrap();
    }

    let mut cmd = Command::new(&qemu);
    cmd.arg("-drive")
        .arg(format!("format=raw,file={}", bios_image))
        .arg("-serial")
        .arg("stdio")
        .arg("-m")
        .arg("256M")
        .arg("-device")
        .arg("virtio-blk-pci,drive=hd0")
        .arg("-drive")
        .arg("id=hd0,if=none,format=raw,file=disk.img")
        .arg("-no-reboot")
        .arg("-no-shutdown");

    // Create a dummy disk image if not exists
    if !std::path::Path::new("disk.img").exists() {
        use std::io::Write;
        let mut file = std::fs::File::create("disk.img").unwrap();
        // 32 MB disk
        file.set_len(32 * 1024 * 1024).unwrap();
        // Write some recognizable data at the beginning
        file.write_all(b"Hello VirtIO!").unwrap();
    }

    println!("Running: {:?}", cmd);

    let status = cmd
        .status()
        .expect("Failed to launch QEMU. Install from https://www.qemu.org/download/#windows");

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}
