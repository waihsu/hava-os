# NOVA OS

An x86_64 learning kernel written in Rust.

## Current milestone

The project creates a BIOS boot image from the `kernel` crate. The kernel writes
`NOVA OS booted` to QEMU's serial console and then exits QEMU.

## Build

Build the bootable BIOS image:

```powershell
cargo build
```

Run it in QEMU after QEMU is installed:

```powershell
cargo run
```
