# HAVA OS architecture

## Current layout

```text
hava-os/
├── src/                    # Windows-side builder and QEMU launcher
├── kernel/                 # Code that runs inside the virtual machine
│   └── src/
│       ├── main.rs          # Kernel entry point and startup sequence
│       ├── serial.rs        # COM1 diagnostics
│       ├── framebuffer.rs   # Pixel display output
│       ├── cpu/
│       │   └── gdt.rs       # GDT and emergency double-fault stack
│       ├── console/         # Framebuffer command prompt and parser
│       ├── drivers/
│       │   ├── ata_pio.rs   # Primary IDE sector reads through ATA PIO
│       │   ├── block.rs     # Common sector-based storage interface
│       │   └── keyboard.rs  # PS/2 keyboard input driver
│       ├── interrupts.rs    # CPU exceptions, PIC and timer IRQs
│       ├── qemu.rs          # QEMU test exit mechanism
│       ├── fs/              # Filesystem API and read-only RAM filesystem
│       ├── task/            # Timer-driven round-robin scheduler foundation
│       └── memory/
│           ├── mod.rs       # Memory module boundary
│           └── frame_allocator.rs
│           └── paging.rs    # Active page-table access
│           └── heap.rs      # Dynamic heap mapping and allocator
├── build.rs                 # Creates the bootable BIOS image
└── docs/                    # Design notes and learning records
```

## Module ownership

Each module owns one responsibility. `main.rs` coordinates modules but should
not contain device drivers or allocation algorithms.

| Area | Future modules |
| --- | --- |
| `memory/` | paging, heap, address spaces |
| `interrupts/` | GDT, IDT, exceptions, timer, keyboard |
| `drivers/` | serial, framebuffer, keyboard, ATA PIO, block-device API |
| `task/` | scheduler, processes, synchronization |
| `fs/` | RAM fallback plus HAVA disk filesystem reader, future FAT prototype |
| `task/` | task metadata, scheduling policy, future context switching |

New features should be added to their owning module and exposed through a
small public API. Hardware-specific details stay behind that API so the kernel
startup code remains readable.

## Disk filesystem prototype

`build.rs` appends a `HAVAFS01` region to the end of the boot image,
followed by reserved writable data sectors and one dedicated scratch sector.
It contains a sector-aligned header, a tiny fixed directory, and file data.
`fs/diskfs.rs` discovers that header by scanning the final disk sectors through
the ATA PIO block driver. This keeps bootloader data untouched and gives `ls`
and `cat` a real disk-backed implementation before a full FAT parser exists.
`disk write` writes only the final scratch sector, then reads it back to verify
the ATA PIO write path. `write NAME text` creates one user file in the reserved
data area and `rm NAME` removes its directory entry. Built-in image files stay
read-only. Deleted user-file sectors are detected as free and reused by the
next `write` operation.

## Scheduler foundation

The PIT timer calls `task::on_timer_tick` every tick. Every ten ticks the
round-robin scheduler selects the next logical task (`kernel`, `console`, or
`idle`). `sleep SECONDS` marks the console task as sleeping until its timer
deadline; keyboard processing resumes after the timer wakes it. The task table
and scheduling policy are ready. `console` and `idle` each have a dedicated
16 KiB aligned static stack reservation. A boot-time probe saves the
callee-saved CPU registers and stack pointer, runs on a separate private stack,
then restores the original context. This verified primitive is connected to
scheduler-selected console and idle task bodies.

The current scheduler now runs `kernel`, `console`, and `idle` task bodies on
their respective contexts. Every ten PIT ticks requests a reschedule; each
task yields only from its normal task stack, avoiding unsafe stack switches
inside an interrupt handler.
