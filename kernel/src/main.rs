#![no_std]
#![no_main]

use bootloader_api::{entry_point, BootInfo};
use core::fmt::Write;
use uart_16550::backend::PioBackend;
use uart_16550::{Config, Uart16550Tty};

#[repr(u32)]
enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

fn serial() -> Uart16550Tty<PioBackend> {
    unsafe { Uart16550Tty::new_port(0x3f8, Config::default()) }
        .expect("valid serial port configuration")
}

fn exit_qemu(code: QemuExitCode) -> ! {
    use x86_64::instructions::{nop, port::Port};

    unsafe {
        Port::new(0xf4).write(code as u32);
    }
    loop {
        nop();
    }
}

entry_point!(kernel_main);

fn kernel_main(_boot_info: &'static mut BootInfo) -> ! {
    writeln!(serial(), "NOVA OS booted").expect("serial output failed");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(serial(), "KERNEL PANIC: {info}");
    exit_qemu(QemuExitCode::Failed);
}
