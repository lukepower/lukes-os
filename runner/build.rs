use std::path::PathBuf;

fn main() {
    // Path to the kernel binary (built for x86_64-unknown-none)
    let kernel_path = {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.parent().unwrap();
        workspace_root
            .join("target")
            .join("x86_64-unknown-none")
            .join("debug")
            .join("kernel")
    };

    println!("cargo:rerun-if-changed={}", kernel_path.display());

    // Create BIOS boot image
    let bios_path = workspace_root_dir()
        .join("target")
        .join("rustos-bios.img");

    let uefi_path = workspace_root_dir()
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

fn workspace_root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}
