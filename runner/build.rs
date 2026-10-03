use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();

    println!("cargo:rerun-if-changed={}", workspace_root.join("kernel").join("src").display());
    println!("cargo:rerun-if-changed={}", workspace_root.join("user").display());

    // 1. Compile User Space Applications
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
        .status();
    if let Ok(st) = user_status {
        if !st.success() {
            panic!("Failed to build user applications");
        }
    }

    // 2. Compile Kernel
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
        .status();
    if let Ok(st) = kernel_status {
        if !st.success() {
            panic!("Failed to build kernel");
        }
    }

    // Path to the kernel binary (built for x86_64-unknown-none)
    let kernel_path = workspace_root
        .join("target")
        .join("x86_64-unknown-none")
        .join("release")
        .join("kernel");

    // Create BIOS boot image
    let bios_path = workspace_root
        .join("target")
        .join("rustos-bios.img");

    let uefi_path = workspace_root
        .join("target")
        .join("rustos-uefi.img");

    if kernel_path.exists() {
        bootloader::BiosBoot::new(&kernel_path)
            .create_disk_image(&bios_path)
            .expect("Failed to create BIOS disk image");

        bootloader::UefiBoot::new(&kernel_path)
            .create_disk_image(&uefi_path)
            .expect("Failed to create UEFI disk image");

        // Pass the image paths to the main binary via env vars
        println!(
            "cargo:rustc-env=BIOS_IMAGE={}",
            bios_path.display()
        );
        println!(
            "cargo:rustc-env=UEFI_IMAGE={}",
            uefi_path.display()
        );
    } else {
        println!("cargo:warning=Kernel binary not found at {}. Build the kernel first.", kernel_path.display());
    }
}
