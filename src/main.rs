use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let image = env!("BIOS_IMAGE");
    let qemu = env::var_os("QEMU_SYSTEM_X86_64").map(PathBuf::from).unwrap_or_else(|| {
        let default_path = PathBuf::from(r"C:\Program Files\qemu\qemu-system-x86_64.exe");
        if default_path.exists() {
            default_path
        } else {
            PathBuf::from("qemu-system-x86_64")
        }
    });

    let status = Command::new(qemu)
        .args(["-drive", &format!("format=raw,file={image}")])
        .args(["-serial", "stdio", "-display", "none"])
        .args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"])
        .status()
        .expect("failed to start QEMU; install QEMU and ensure it is on PATH");

    // `isa-debug-exit` returns `(guest_code << 1) | 1` to the host.
    let code = match status.code() {
        Some(0x21) => 0,
        Some(0x23) => 1,
        Some(other) => other,
        None => 1,
    };
    std::process::exit(code);
}
