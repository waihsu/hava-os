# HAVA OS

An x86_64 learning kernel written in Rust.

## Current milestone

The project creates a BIOS boot image from the `kernel` crate. The kernel writes
`HAVA OS booted` to QEMU's serial console and then exits QEMU.

## Build

Build the bootable BIOS image:

```powershell
cargo build
```

Run it in QEMU after QEMU is installed:

```powershell
cargo run
```

Use `cargo run -- --hold` to keep the QEMU graphics window open after boot.

To test keyboard input, run the hold command, click the QEMU window, and press
letter keys. The kernel's main loop consumes each decoded US-layout key and
prints it as `Input: x`.
