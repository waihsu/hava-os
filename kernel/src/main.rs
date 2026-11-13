#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
use bootloader_api::{entry_point, BootInfo};
#[cfg(target_os = "none")]
use core::fmt::Write;
#[cfg(target_os = "none")]
use uart_16550::backend::PioBackend;
#[cfg(target_os = "none")]
use uart_16550::{Config, Uart16550Tty};

#[cfg(target_os = "none")]
#[repr(u32)]
enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

#[cfg(target_os = "none")]
fn serial() -> Uart16550Tty<PioBackend> {
    unsafe { Uart16550Tty::new_port(0x3f8, Config::default()) }
        .expect("valid serial port configuration")
}

#[cfg(target_os = "none")]
fn exit_qemu(code: QemuExitCode) -> ! {
    use x86_64::instructions::{nop, port::Port};

    unsafe {
        Port::new(0xf4).write(code as u32);
    }
    loop {
        nop();
    }
}

#[cfg(target_os = "none")]
entry_point!(kernel_main);

#[cfg(target_os = "none")]
fn kernel_main(_boot_info: &'static mut BootInfo) -> ! {
    writeln!(serial(), "NOVA OS booted").expect("serial output failed");
    exit_qemu(QemuExitCode::Success);
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(serial(), "KERNEL PANIC: {info}");
    exit_qemu(QemuExitCode::Failed);
}

// Lets Rust Analyzer check this crate on the Windows host. This function is
// never included in the bare-metal kernel binary.
#[cfg(not(target_os = "none"))]
fn main() {}
