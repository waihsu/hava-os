#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
#![cfg_attr(target_os = "none", feature(abi_x86_interrupt))]

#[cfg(target_os = "none")]
extern crate alloc;

#[cfg(target_os = "none")]
mod framebuffer;
#[cfg(target_os = "none")]
mod console;
#[cfg(target_os = "none")]
mod cpu;
#[cfg(target_os = "none")]
mod drivers;
#[cfg(target_os = "none")]
mod fs;
#[cfg(target_os = "none")]
mod interrupts;
#[cfg(target_os = "none")]
mod memory;
#[cfg(target_os = "none")]
mod qemu;
#[cfg(target_os = "none")]
mod serial;
#[cfg(target_os = "none")]
mod task;

#[cfg(target_os = "none")]
use bootloader_api::{entry_point, BootInfo};
#[cfg(target_os = "none")]
use bootloader_api::config::{BootloaderConfig, Mapping};
#[cfg(target_os = "none")]
use core::fmt::Write;
#[cfg(target_os = "none")]
use x86_64::structures::paging::FrameAllocator;
#[cfg(target_os = "none")]
use x86_64::structures::paging::{RecursivePageTable, Translate};
#[cfg(target_os = "none")]
use x86_64::VirtAddr;

#[cfg(target_os = "none")]
const BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.page_table_recursive = Some(Mapping::Dynamic);
    config
};

#[cfg(target_os = "none")]
entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

#[cfg(target_os = "none")]
static mut FRAMEBUFFER_CONSOLE: Option<console::Console<'static>> = None;
#[cfg(target_os = "none")]
static mut CONSOLE_MEMORY: Option<console::MemorySummary> = None;

#[cfg(target_os = "none")]
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    print_memory_map(boot_info);
    let memory_summary = console::MemorySummary::from_boot_info(boot_info);

    cpu::gdt::init();
    interrupts::init();
    task::init();
    writeln!(serial::port(), "Interrupts: IDT loaded").expect("serial output failed");
    writeln!(serial::port(), "Scheduler: kernel, console, and idle tasks registered")
        .expect("serial output failed");
    let context_switch_verified = task::demonstrate_context_switch();
    writeln!(
        serial::port(),
        "Scheduler: private-stack context switch {}",
        if context_switch_verified { "verified" } else { "failed" }
    )
    .expect("serial output failed");
    interrupts::trigger_breakpoint();
    interrupts::start_timer(100);
    interrupts::wait_for_ticks(5);
    writeln!(
        serial::port(),
        "Timer: {} ticks received at 100 Hz",
        interrupts::ticks()
    )
    .expect("serial output failed");
    interrupts::wait_for_ticks(25);
    let (_, _, scheduler_switches) = task::snapshot();
    writeln!(
        serial::port(),
        "Scheduler: {} switches before task runtime; time-slice requests armed at {} ticks",
        scheduler_switches,
        interrupts::ticks()
    )
    .expect("serial output failed");
    writeln!(serial::port(), "Keyboard: IRQ1 driver ready (use --hold to test)")
        .expect("serial output failed");

    let recursive_index = *boot_info
        .recursive_index
        .as_ref()
        .expect("bootloader did not create a recursive page-table mapping");
    let kernel_image_offset = boot_info.kernel_image_offset;
    let mut allocator = unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_regions) };
    let mut mapper = unsafe { memory::paging::init(recursive_index) };

    demonstrate_frame_allocation(&mut allocator);
    demonstrate_paging(&mut mapper, kernel_image_offset);
    demonstrate_new_mapping(&mut mapper, &mut allocator);
    demonstrate_heap(&mut mapper, &mut allocator);
    demonstrate_disk_info();
    demonstrate_boot_disk();
    demonstrate_disk_filesystem();
    demonstrate_writable_disk_filesystem();
    writeln!(serial::port(), "HAVA OS ready").expect("serial output failed");
    qemu::signal_exit(qemu::ExitCode::Success);
    unsafe {
        (&raw mut FRAMEBUFFER_CONSOLE).write(boot_info.framebuffer.as_mut().map(console::Console::new));
        (&raw mut CONSOLE_MEMORY).write(Some(memory_summary));
    }
    task::initialize_task_contexts(console_task_entry);
    kernel_event_loop();
}

/// Runs after startup. Normal boot checks exit immediately; `--hold` keeps
/// QEMU open so queued keyboard input can be consumed here.
#[cfg(target_os = "none")]
fn kernel_event_loop() -> ! {
    loop {
        task::yield_if_requested();
        x86_64::instructions::hlt();
    }
}

/// The console task owns keyboard input and framebuffer output on its private stack.
#[cfg(target_os = "none")]
extern "C" fn console_task_entry() -> ! {
    loop {
        if task::is_console_awake()
            && let Some(character) = drivers::keyboard::pop_character()
        {
            unsafe {
                let console_slot = &mut *(&raw mut FRAMEBUFFER_CONSOLE);
                let memory = (*(&raw const CONSOLE_MEMORY))
                    .expect("console task memory summary was not initialized");
                if let Some(console) = console_slot.as_mut() {
                    console.handle_input(character, memory);
                }
            }
            writeln!(serial::port(), "Input: {character}").expect("serial output failed");
        }
        if !task::is_console_awake() {
            task::yield_now();
        } else {
            task::yield_if_requested();
            x86_64::instructions::hlt();
        }
    }
}

/// The idle task yields CPU time until the next hardware interrupt.
#[cfg(target_os = "none")]
fn run_idle_task() -> ! {
    loop {
        x86_64::instructions::hlt();
        task::yield_if_requested();
    }
}

/// Translates the kernel's own virtual base address through the active page tables.
#[cfg(target_os = "none")]
fn demonstrate_paging(mapper: &mut RecursivePageTable, kernel_image_offset: u64) {
    let virtual_address = VirtAddr::new(kernel_image_offset);
    let physical_address = mapper
        .translate_addr(virtual_address)
        .expect("kernel image base must be mapped");

    writeln!(
        serial::port(),
        "Paging: virtual {:#x} maps to physical {:#x}",
        virtual_address.as_u64(),
        physical_address.as_u64()
    )
    .expect("serial output failed");
}

/// Maps a fresh virtual page, stores a value in it, and reads it back.
#[cfg(target_os = "none")]
fn demonstrate_new_mapping(
    mapper: &mut RecursivePageTable,
    allocator: &mut memory::BootInfoFrameAllocator,
) {
    let (virtual_address, value) = unsafe { memory::paging::map_example_page(mapper, allocator) };
    writeln!(
        serial::port(),
        "New mapping: wrote {:#x}, read {:#x} at virtual {:#x}",
        memory::paging::EXAMPLE_VALUE,
        value,
        virtual_address.as_u64()
    )
    .expect("serial output failed");
}

/// Initializes the dynamic heap and proves that `Box` can allocate memory.
#[cfg(target_os = "none")]
fn demonstrate_heap(
    mapper: &mut RecursivePageTable,
    allocator: &mut memory::BootInfoFrameAllocator,
) {
    memory::heap::init(mapper, allocator).expect("heap page mapping failed");

    let value = alloc::boxed::Box::new(40_u64);
    let result = *value + 2;
    writeln!(serial::port(), "Heap: Box allocation succeeded ({result})")
        .expect("serial output failed");
}

/// Reads the boot disk's first sector through the ATA block driver.
#[cfg(target_os = "none")]
fn demonstrate_boot_disk() {
    use crate::drivers::block::BlockDevice;

    let mut sector = [0_u8; crate::drivers::block::SECTOR_SIZE];
    let result = drivers::ata_pio::AtaPio::primary_master().read_sector(0, &mut sector);
    match result {
        Ok(()) if sector[510] == 0x55 && sector[511] == 0xaa => {
            writeln!(serial::port(), "Disk: boot sector read; signature 55 AA valid")
        }
        Ok(()) => writeln!(serial::port(), "Disk: boot sector read; signature missing"),
        Err(error) => writeln!(serial::port(), "Disk: boot sector read failed: {error:?}"),
    }
    .expect("serial output failed");
}

/// Queries the QEMU IDE controller for its model and capacity.
#[cfg(target_os = "none")]
fn demonstrate_disk_info() {
    match drivers::ata_pio::AtaPio::primary_master().identify() {
        Ok(info) => writeln!(
            serial::port(),
            "Disk: model {:?}; {} sectors ({} KiB)",
            core::str::from_utf8(&info.model).unwrap_or("unknown"),
            info.sectors,
            info.sectors as u64 / 2
        ),
        Err(error) => writeln!(serial::port(), "Disk: identify failed: {error:?}"),
    }
    .expect("serial output failed");
}

/// Mounts the append-only HAVA filesystem and reads one file from the disk.
#[cfg(target_os = "none")]
fn demonstrate_disk_filesystem() {
    let mut disk = drivers::ata_pio::AtaPio::primary_master();
    let result = disk
        .identify()
        .map_err(fs::DiskFsError::from)
        .and_then(|info| fs::DiskFileSystem::mount(&mut disk, info.sectors as u64));

    match result {
        Ok(filesystem) => {
            let mut contents = [0_u8; drivers::block::SECTOR_SIZE];
            match filesystem.read(&mut disk, "README", &mut contents) {
                Ok(length) => {
                    writeln!(
                        serial::port(),
                        "Disk FS: mounted {} files; README: {}",
                        filesystem.file_count(),
                        core::str::from_utf8(&contents[..length]).unwrap_or("invalid UTF-8")
                    )
                    .expect("serial output failed");
                    let manual = filesystem.find("MANUAL").expect("MANUAL must be on boot disk");
                    let sector_count = manual.length().div_ceil(drivers::block::SECTOR_SIZE);
                    for file_sector in 0..sector_count {
                        filesystem
                            .read_file_sector(&mut disk, manual, file_sector, &mut contents)
                            .expect("MANUAL sector read failed");
                    }
                    writeln!(
                        serial::port(),
                        "Disk FS: streamed MANUAL ({} bytes across {} sectors)",
                        manual.length(),
                        sector_count
                    )
                }
                Err(error) => writeln!(serial::port(), "Disk FS: README read failed: {error:?}"),
            }
        }
        Err(error) => writeln!(serial::port(), "Disk FS: mount failed: {error:?}"),
    }
    .expect("serial output failed");
}

/// Proves that a user file can be written, read, and removed safely.
#[cfg(target_os = "none")]
fn demonstrate_writable_disk_filesystem() {
    const NAME: &str = "SELFTEST";
    const CONTENTS: &[u8] = b"HAVA writable filesystem self-test";

    let mut disk = drivers::ata_pio::AtaPio::primary_master();
    let result = disk
        .identify()
        .map_err(fs::DiskFsError::from)
        .and_then(|info| fs::DiskFileSystem::mount(&mut disk, info.sectors as u64));
    let Ok(mut filesystem) = result else {
        writeln!(serial::port(), "Disk FS: writable self-test mount failed")
            .expect("serial output failed");
        return;
    };
    let mut buffer = [0_u8; drivers::block::SECTOR_SIZE];
    let verified = filesystem
        .write_file(&mut disk, NAME, CONTENTS)
        .is_ok_and(|first_sector| {
            filesystem.remove_file(&mut disk, NAME).is_ok()
                && filesystem.write_file(&mut disk, NAME, CONTENTS).is_ok_and(|second_sector| {
                    first_sector == second_sector
                        && filesystem.read(&mut disk, NAME, &mut buffer)
                            .is_ok_and(|length| &buffer[..length] == CONTENTS)
                        && filesystem.remove_file(&mut disk, NAME).is_ok()
                })
        });
    writeln!(
        serial::port(),
        "Disk FS: writable self-test {}",
        if verified { "verified" } else { "failed" }
    )
    .expect("serial output failed");
}


/// Shows the first three safe-to-use 4 KiB physical frames.
#[cfg(target_os = "none")]
fn demonstrate_frame_allocation(allocator: &mut memory::BootInfoFrameAllocator) {
    let mut port = serial::port();
    writeln!(port, "First usable physical frames:").expect("serial output failed");

    for _ in 0..3 {
        let frame = allocator
            .allocate_frame()
            .expect("bootloader did not report enough usable memory");
        writeln!(port, "  {:#x}", frame.start_address().as_u64())
            .expect("serial output failed");
    }
}

/// Print the firmware memory map supplied by the bootloader.
#[cfg(target_os = "none")]
fn print_memory_map(boot_info: &BootInfo) {
    let mut port = serial::port();
    writeln!(port, "HAVA OS memory map:").expect("serial output failed");

    for region in boot_info.memory_regions.iter() {
        writeln!(
            port,
            "  {:#010x}..{:#010x}  {:?}",
            region.start,
            region.end,
            region.kind
        )
        .expect("serial output failed");
    }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(serial::port(), "KERNEL PANIC: {info}");
    qemu::exit(qemu::ExitCode::Failed);
}

#[cfg(target_os = "none")]
#[alloc_error_handler]
fn allocation_error(layout: core::alloc::Layout) -> ! {
    let _ = writeln!(serial::port(), "HEAP ERROR: allocation of {} bytes failed", layout.size());
    qemu::exit(qemu::ExitCode::Failed);
}

// Lets Rust Analyzer check this crate on the Windows host. This function is
// never included in the bare-metal kernel binary.
#[cfg(not(target_os = "none"))]
fn main() {}
