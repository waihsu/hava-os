use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let kernel = PathBuf::from(std::env::var_os("CARGO_BIN_FILE_KERNEL_kernel").unwrap());
    let bios_image = out_dir.join("nova-os-bios.img");

    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&bios_image)
        .expect("failed to create BIOS disk image");

    println!("cargo:rustc-env=BIOS_IMAGE={}", bios_image.display());
}
