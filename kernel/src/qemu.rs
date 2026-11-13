//! QEMU-specific exit support used by automated boot checks.

use x86_64::instructions::{hlt, port::Port};

#[repr(u32)]
pub enum ExitCode {
    Success = 0x10,
    Failed = 0x11,
}

/// Sends a code to QEMU's optional debug-exit device.
///
/// When launched with the device this ends the normal automated boot check.
/// In `--hold` mode the device is absent, so execution continues into the
/// kernel event loop.
pub fn signal_exit(code: ExitCode) {
    unsafe {
        Port::new(0xf4).write(code as u32);
    }
}

pub fn exit(code: ExitCode) -> ! {
    signal_exit(code);
    loop {
        hlt();
    }
}
