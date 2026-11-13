//! CPU exception and hardware interrupt setup.

use core::fmt::Write;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use pic8259::ChainedPics;
use spin::{Mutex, Once};
use x86_64::{
    instructions::{hlt, interrupts::{enable, int3}, port::Port},
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

const PIC_1_OFFSET: u8 = 32;
const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
const TIMER_VECTOR: u8 = PIC_1_OFFSET;
const KEYBOARD_VECTOR: u8 = PIC_1_OFFSET + 1;
const ATA_PRIMARY_IRQ_MASK: u8 = 1 << 6;
const PIT_FREQUENCY_HZ: u32 = 1_193_182;

static IDT: Once<InterruptDescriptorTable> = Once::new();
static PICS: Mutex<ChainedPics> = Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });
static TICKS: AtomicU64 = AtomicU64::new(0);
static TIMER_FREQUENCY_HZ: AtomicU32 = AtomicU32::new(0);

/// Loads the interrupt descriptor table for the current CPU.
pub fn init() {
    IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(crate::cpu::gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt[TIMER_VECTOR].set_handler_fn(timer_interrupt_handler);
        idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_interrupt_handler);
        idt
    })
    .load();
}

/// Enables IRQ0 from the programmable interval timer at the requested rate.
pub fn start_timer(frequency_hz: u32) {
    let divisor = PIT_FREQUENCY_HZ / frequency_hz;
    assert!(divisor > 0 && divisor <= u16::MAX.into());

    unsafe {
        let mut command = Port::<u8>::new(0x43);
        let mut data = Port::<u8>::new(0x40);
        command.write(0x36); // channel 0, low/high byte, rate generator
        data.write((divisor & 0xff) as u8);
        data.write((divisor >> 8) as u8);
        let mut pics = PICS.lock();
        pics.initialize();

        // The ATA PIO driver polls status instead of handling IRQ14. Masking
        // it prevents an unconfigured IDT vector while an IDE command runs.
        let masks = pics.read_masks();
        pics.write_masks(masks[0], masks[1] | ATA_PRIMARY_IRQ_MASK);
    }

    TIMER_FREQUENCY_HZ.store(frequency_hz, Ordering::Relaxed);
    enable();
}

/// Sleeps the CPU between timer interrupts until the requested tick count arrives.
pub fn wait_for_ticks(target: u64) {
    while ticks() < target {
        hlt();
    }
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Returns the elapsed time measured by the programmed PIT.
pub fn uptime_millis() -> u64 {
    let frequency_hz = TIMER_FREQUENCY_HZ.load(Ordering::Relaxed) as u64;
    if frequency_hz == 0 {
        0
    } else {
        ticks() * 1_000 / frequency_hz
    }
}

/// Runs the CPU's breakpoint exception instruction as a safe first test.
pub fn trigger_breakpoint() {
    int3();
}

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {
    let _ = writeln!(crate::serial::port(), "Interrupt: breakpoint handled");
}

extern "x86-interrupt" fn double_fault_handler(
    _stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    let _ = writeln!(crate::serial::port(), "CPU ERROR: double fault handled on emergency stack");
    crate::qemu::exit(crate::qemu::ExitCode::Failed);
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let ticks = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    crate::task::on_timer_tick(ticks);
    unsafe {
        PICS.lock().notify_end_of_interrupt(TIMER_VECTOR);
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    crate::drivers::keyboard::handle_interrupt();
    unsafe {
        PICS.lock().notify_end_of_interrupt(KEYBOARD_VECTOR);
    }
}
