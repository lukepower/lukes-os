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
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();

    println!("=== Compiling User Space Applications ===");
    let user_status = Command::new("cargo")
        .args(&[
            "build",
            "--target",
            "x86_64-unknown-none",
            "--release",
            "--manifest-path",
            "user/Cargo.toml",
        ])
        .current_dir(workspace_root)
        .status()
        .expect("Failed to execute cargo build for user workspace");
    if !user_status.success() {
        eprintln!("Error compiling user workspace!");
        std::process::exit(user_status.code().unwrap_or(1));
    }

    println!("=== Compiling Kernel ===");
    let kernel_status = Command::new("cargo")
        .args(&[
            "build",
            "--target",
            "x86_64-unknown-none",
            "--release",
            "--package",
            "kernel",
        ])
        .current_dir(workspace_root)
        .status()
        .expect("Failed to execute cargo build for kernel");
    if !kernel_status.success() {
        eprintln!("Error compiling kernel!");
        std::process::exit(kernel_status.code().unwrap_or(1));
    }

    // Invoke cargo build on runner so its build.rs regenerates the boot images
    println!("=== Packaging Boot Disk Images ===");
    let package_status = Command::new("cargo")
        .args(&["build", "--package", "runner"])
        .current_dir(workspace_root)
        .status()
        .expect("Failed to package boot disk images");
    if !package_status.success() {
        eprintln!("Error packaging boot disk images!");
        std::process::exit(package_status.code().unwrap_or(1));
    }

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
        .arg("-smp")
        .arg("4")
        .arg("-no-reboot")
        .arg("-no-shutdown");
    println!("Running: {:?}", cmd);

    let status = cmd
        .status()
        .expect("Failed to launch QEMU. Install from https://www.qemu.org/download/#windows");

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}
