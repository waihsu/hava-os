use std::{fs::OpenOptions, io::{Seek, SeekFrom, Write}, path::PathBuf};

const SECTOR_SIZE: usize = 512;
const FILE_NAME_BYTES: usize = 24;
const DIRECTORY_OFFSET: usize = 32;
const DIRECTORY_ENTRY_SIZE: usize = 32;
const WRITABLE_DATA_SECTORS: usize = 8;

fn append_hava_filesystem(image: &PathBuf) {
    let files = [
        ("README", "Welcome to HAVA OS. This file came from the boot disk."),
        ("LICENSE", "HAVA OS is a learning kernel project."),
        ("MANUAL", concat!(
            "HAVA OS disk filesystem manual.\n",
            "This text intentionally spans more than one 512-byte sector.\n",
            "The ATA PIO driver reads each sector from the QEMU boot disk.\n",
            "The disk filesystem locates its header near the end of that disk.\n",
            "A directory entry stores a file name, first sector, and byte length.\n",
            "The console reuses one small buffer while it streams every sector.\n",
            "That means a file need not fit in RAM as one contiguous allocation.\n",
            "Later, a real filesystem can keep this streaming read behavior.\n",
            "This is the boundary between storage hardware and filesystem logic.\n",
            "End of the HAVA OS disk filesystem manual.\n",
        )),
    ];
    let mut image_file = OpenOptions::new().read(true).write(true).open(image)
        .expect("failed to reopen BIOS disk image");
    let image_len = image_file.metadata().expect("failed to read image metadata").len() as usize;
    let padding = (SECTOR_SIZE - image_len % SECTOR_SIZE) % SECTOR_SIZE;
    image_file.seek(SeekFrom::End(0)).expect("failed to seek BIOS image");
    image_file.write_all(&vec![0; padding]).expect("failed to align BIOS image");

    let mut header = [0_u8; SECTOR_SIZE];
    header[..8].copy_from_slice(b"HAVAFS01");
    header[8] = 1;
    header[9] = files.len() as u8;
    let mut next_sector = 1_u32;
    let mut file_data = Vec::new();

    for (index, (name, contents)) in files.iter().enumerate() {
        let name = name.as_bytes();
        let contents = contents.as_bytes();
        assert!(name.len() <= FILE_NAME_BYTES, "filesystem name is too long");
        assert!(contents.len() <= u16::MAX.into(), "filesystem file is too large");

        let entry = DIRECTORY_OFFSET + index * DIRECTORY_ENTRY_SIZE;
        header[entry] = name.len() as u8;
        header[entry + 1..entry + 1 + name.len()].copy_from_slice(name);
        header[entry + 25..entry + 29].copy_from_slice(&next_sector.to_le_bytes());
        header[entry + 29..entry + 31].copy_from_slice(&(contents.len() as u16).to_le_bytes());

        file_data.extend_from_slice(contents);
        file_data.resize(file_data.len().next_multiple_of(SECTOR_SIZE), 0);
        next_sector += contents.len().div_ceil(SECTOR_SIZE) as u32;
    }

    header[10..14].copy_from_slice(&next_sector.to_le_bytes());
    header[14..18].copy_from_slice(&next_sector.to_le_bytes());

    image_file.write_all(&header).expect("failed to append filesystem header");
    image_file.write_all(&file_data).expect("failed to append filesystem files");
    image_file
        .write_all(&vec![0; WRITABLE_DATA_SECTORS * SECTOR_SIZE])
        .expect("failed to append writable filesystem sectors");
    // The final sector is deliberately reserved for safe ATA write testing.
    image_file.write_all(&[0; SECTOR_SIZE]).expect("failed to append scratch sector");
}

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let kernel = PathBuf::from(std::env::var_os("CARGO_BIN_FILE_KERNEL_kernel").unwrap());
    let bios_image = out_dir.join("hava-os-bios.img");

    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&bios_image)
        .expect("failed to create BIOS disk image");
    append_hava_filesystem(&bios_image);

    println!("cargo:rustc-env=BIOS_IMAGE={}", bios_image.display());
}
