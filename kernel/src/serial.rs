//! COM1 serial output for early kernel diagnostics.

use uart_16550::backend::PioBackend;
use uart_16550::{Config, Uart16550Tty};

pub fn port() -> Uart16550Tty<PioBackend> {
    unsafe { Uart16550Tty::new_port(0x3f8, Config::default()) }
        .expect("valid serial port configuration")
}
