//! PS/2 keyboard input through IRQ1.

use core::cell::UnsafeCell;
use pc_keyboard::{DecodedKey, HandleControl, PS2Keyboard, ScancodeSet1, layouts};
use spin::Mutex;
use x86_64::instructions::port::Port;

static KEYBOARD: Mutex<PS2Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(PS2Keyboard::new(
    ScancodeSet1::new(),
    layouts::Us104Key,
    HandleControl::Ignore,
));

const INPUT_QUEUE_CAPACITY: usize = 128;

struct InputQueue {
    buffer: UnsafeCell<[u8; INPUT_QUEUE_CAPACITY]>,
    read: UnsafeCell<usize>,
    write: UnsafeCell<usize>,
}

unsafe impl Sync for InputQueue {}

static INPUT_QUEUE: InputQueue = InputQueue {
    buffer: UnsafeCell::new([0; INPUT_QUEUE_CAPACITY]),
    read: UnsafeCell::new(0),
    write: UnsafeCell::new(0),
};

/// Reads one PS/2 scancode and queues printable key presses for the main loop.
pub fn handle_interrupt() {
    let scancode = unsafe { Port::<u8>::new(0x60).read() };
    let mut keyboard = KEYBOARD.lock();

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(DecodedKey::Unicode(character)) = keyboard.process_keyevent(key_event) {
            if character.is_ascii() {
                push_character(character as u8);
            }
        }
    }
}

/// Returns the next decoded ASCII character, if one is waiting.
pub fn pop_character() -> Option<char> {
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let read = *INPUT_QUEUE.read.get();
        let write = *INPUT_QUEUE.write.get();
        if read == write {
            return None;
        }

        let byte = (*INPUT_QUEUE.buffer.get())[read];
        *INPUT_QUEUE.read.get() = (read + 1) % INPUT_QUEUE_CAPACITY;
        Some(byte as char)
    })
}

fn push_character(byte: u8) {
    unsafe {
        let read = *INPUT_QUEUE.read.get();
        let write = *INPUT_QUEUE.write.get();
        let next_write = (write + 1) % INPUT_QUEUE_CAPACITY;

        // Leave one slot empty to distinguish a full queue from an empty one.
        if next_write != read {
            (*INPUT_QUEUE.buffer.get())[write] = byte;
            *INPUT_QUEUE.write.get() = next_write;
        }
    }
}
