use bootloader_api::{BootInfo, info::FrameBuffer};

use crate::{
    console::command::{self, Command},
    framebuffer::{self, BACKGROUND, FOREGROUND},
    drivers::block::{BlockDevice, SECTOR_SIZE}, fs::FileSystem,
};

const SCALE: usize = 2;
const CELL_WIDTH: usize = 8 * SCALE;
const CELL_HEIGHT: usize = 8 * SCALE;
const COMMAND_CAPACITY: usize = 96;

#[derive(Clone, Copy)]
pub struct MemorySummary {
    usable_regions: usize,
    usable_bytes: u64,
}

impl MemorySummary {
    pub fn from_boot_info(boot_info: &BootInfo) -> Self {
        let mut usable_regions = 0;
        let mut usable_bytes = 0;
        for region in boot_info.memory_regions.iter() {
            if region.kind == bootloader_api::info::MemoryRegionKind::Usable {
                usable_regions += 1;
                usable_bytes += region.end - region.start;
            }
        }
        Self { usable_regions, usable_bytes }
    }
}

pub struct Console<'a> {
    framebuffer: &'a mut FrameBuffer,
    column: usize,
    row: usize,
    columns: usize,
    rows: usize,
    command: [u8; COMMAND_CAPACITY],
    command_len: usize,
}

impl<'a> Console<'a> {
    pub fn new(framebuffer: &'a mut FrameBuffer) -> Self {
        let info = framebuffer.info();
        let mut console = Self {
            framebuffer,
            column: 0,
            row: 0,
            columns: info.width / CELL_WIDTH,
            rows: info.height / CELL_HEIGHT,
            command: [0; COMMAND_CAPACITY],
            command_len: 0,
        };
        console.clear();
        console.write_line("HAVA OS console");
        console.write_line("Type help for commands.");
        console.prompt();
        console
    }

    pub fn handle_input(&mut self, character: char, memory: MemorySummary) {
        match character {
            '\n' | '\r' => self.submit(memory),
            '\u{8}' => self.backspace(),
            character if character.is_ascii_graphic() || character == ' ' => self.push(character),
            _ => {}
        }
    }

    fn submit(&mut self, memory: MemorySummary) {
        self.newline();
        let input = unsafe { core::str::from_utf8_unchecked(&self.command[..self.command_len]) };
        match command::parse(input) {
            Command::Help => self.write_line("Commands: help, mem, uptime, tasks, sleep, echo, ls, cat, write, rm, fs info, disk, note, clear"),
            Command::Memory => {
                self.write_line("Memory summary:");
                self.write_decimal("  usable regions: ", memory.usable_regions as u64);
                self.write_decimal("  usable KiB: ", memory.usable_bytes / 1024);
            }
            Command::Uptime => {
                let milliseconds = crate::interrupts::uptime_millis();
                self.write_decimal("Uptime seconds: ", milliseconds / 1_000);
                self.write_decimal("Uptime milliseconds: ", milliseconds % 1_000);
            }
            Command::Echo => self.echo_arguments(),
            Command::ListFiles => self.list_files(),
            Command::Cat => self.read_file(),
            Command::Disk => self.inspect_boot_disk(),
            Command::DiskInfo => self.inspect_disk_info(),
            Command::DiskWrite => self.test_disk_write(),
            Command::Note => self.read_note(),
            Command::NoteWrite => self.write_note(),
            Command::Write => self.write_file(),
            Command::Remove => self.remove_file(),
            Command::FileSystemInfo => self.show_filesystem_info(),
            Command::Tasks => self.show_tasks(),
            Command::Sleep => self.sleep_console(),
            Command::Clear => self.clear(),
            Command::Unknown => self.write_line("Unknown command. Type help."),
            Command::Empty => {}
        }
        self.command_len = 0;
        self.prompt();
    }

    /// Writes the portion after `echo ` without allocating a String.
    fn echo_arguments(&mut self) {
        const ECHO_PREFIX_LEN: usize = 5;
        if self.command_len > ECHO_PREFIX_LEN {
            for index in ECHO_PREFIX_LEN..self.command_len {
                let character = self.command[index] as char;
                self.write_char(character);
            }
        }
        self.newline();
    }

    fn list_files(&mut self) {
        let mut disk = crate::drivers::ata_pio::AtaPio::primary_master();
        let result = disk
            .identify()
            .map_err(crate::fs::DiskFsError::from)
            .and_then(|info| crate::fs::DiskFileSystem::mount(&mut disk, info.sectors as u64));

        match result {
            Ok(filesystem) => {
                self.write_line("Disk files:");
                for file in filesystem.files() {
                    self.write_str("  ");
                    self.write_bytes(file.name());
                    self.newline();
                }
            }
            Err(_) => self.list_ram_files(),
        }
    }

    fn read_file(&mut self) {
        const CAT_PREFIX_LEN: usize = 4;
        let name = unsafe {
            core::str::from_utf8_unchecked(&self.command[CAT_PREFIX_LEN..self.command_len])
        }
        .trim();

        let mut disk = crate::drivers::ata_pio::AtaPio::primary_master();
        let result = disk
            .identify()
            .map_err(crate::fs::DiskFsError::from)
            .and_then(|info| crate::fs::DiskFileSystem::mount(&mut disk, info.sectors as u64));

        match result {
            Ok(filesystem) => {
                let mut contents = [0_u8; SECTOR_SIZE];
                match filesystem.find(name) {
                    Some(file) => {
                        let mut remaining = file.length();
                        for file_sector in 0..remaining.div_ceil(SECTOR_SIZE) {
                            if filesystem.read_file_sector(&mut disk, file, file_sector, &mut contents).is_err() {
                                self.write_line("File read failed.");
                                return;
                            }
                            let bytes_to_write = remaining.min(SECTOR_SIZE);
                            self.write_bytes(&contents[..bytes_to_write]);
                            remaining -= bytes_to_write;
                        }
                        self.newline();
                    }
                    None => self.write_line("File not found."),
                }
            }
            Err(_) => match crate::fs::ROOT.read(name) {
                Some(contents) => self.write_line(contents),
                None => self.write_line("File not found."),
            },
        }
    }

    fn list_ram_files(&mut self) {
        self.write_line("RAM files (disk filesystem unavailable):");
        for file in crate::fs::ROOT.files() {
            self.write_str("  ");
            self.write_line(file.name);
        }
    }

    fn inspect_boot_disk(&mut self) {
        let mut sector = [0_u8; SECTOR_SIZE];
        let result = crate::drivers::ata_pio::AtaPio::primary_master()
            .read_sector(0, &mut sector);
        match result {
            Ok(()) if sector[510] == 0x55 && sector[511] == 0xaa => {
                self.write_line("Boot disk: sector 0 read; signature 55 AA valid.");
            }
            Ok(()) => self.write_line("Boot disk: sector 0 read; signature missing."),
            Err(_) => self.write_line("Boot disk: sector read failed."),
        }
    }

    fn inspect_disk_info(&mut self) {
        match crate::drivers::ata_pio::AtaPio::primary_master().identify() {
            Ok(info) => {
                self.write_str("Disk model: ");
                for character in info.model {
                    if character.is_ascii_graphic() || character == b' ' {
                        self.write_char(character as char);
                    }
                }
                self.newline();
                self.write_decimal("Disk sectors: ", info.sectors as u64);
                self.write_decimal("Disk KiB: ", info.sectors as u64 / 2);
            }
            Err(_) => self.write_line("Disk: identify command failed."),
        }
    }

    fn test_disk_write(&mut self) {
        const MESSAGE: &[u8] = b"HAVA OS scratch-sector write verified.";
        let mut disk = crate::drivers::ata_pio::AtaPio::primary_master();
        let Ok(info) = disk.identify() else {
            self.write_line("Disk: identify command failed.");
            return;
        };
        let scratch_sector = u64::from(info.sectors.saturating_sub(1));
        let mut written = [0_u8; SECTOR_SIZE];
        written[..MESSAGE.len()].copy_from_slice(MESSAGE);
        let mut read_back = [0_u8; SECTOR_SIZE];

        match disk.write_sector(scratch_sector, &written) {
            Ok(()) if disk.read_sector(scratch_sector, &mut read_back).is_ok() && read_back == written => {
                self.write_line("Disk: scratch write and read-back verified.");
            }
            Ok(()) => self.write_line("Disk: scratch write verification failed."),
            Err(_) => self.write_line("Disk: scratch write failed."),
        }
    }

    fn write_note(&mut self) {
        const MAGIC: &[u8; 8] = b"HAVANOTE";
        const PREFIX_LEN: usize = 5;
        let note = &self.command[PREFIX_LEN..self.command_len];
        let mut disk = crate::drivers::ata_pio::AtaPio::primary_master();
        let Ok(info) = disk.identify() else {
            self.write_line("Note: disk identify failed.");
            return;
        };
        let mut sector = [0_u8; SECTOR_SIZE];
        sector[..MAGIC.len()].copy_from_slice(MAGIC);
        sector[8..10].copy_from_slice(&(note.len() as u16).to_le_bytes());
        sector[10..10 + note.len()].copy_from_slice(note);

        match disk.write_sector(u64::from(info.sectors.saturating_sub(1)), &sector) {
            Ok(()) => self.write_line("Note saved to disk."),
            Err(_) => self.write_line("Note save failed."),
        }
    }

    fn read_note(&mut self) {
        const MAGIC: &[u8; 8] = b"HAVANOTE";
        let mut disk = crate::drivers::ata_pio::AtaPio::primary_master();
        let Ok(info) = disk.identify() else {
            self.write_line("Note: disk identify failed.");
            return;
        };
        let mut sector = [0_u8; SECTOR_SIZE];
        if disk.read_sector(u64::from(info.sectors.saturating_sub(1)), &mut sector).is_err() {
            self.write_line("Note read failed.");
            return;
        }
        if &sector[..MAGIC.len()] != MAGIC {
            self.write_line("No saved note.");
            return;
        }
        let length = usize::from(u16::from_le_bytes(sector[8..10].try_into().unwrap()));
        if length > SECTOR_SIZE - 10 {
            self.write_line("Saved note is invalid.");
            return;
        }
        self.write_bytes(&sector[10..10 + length]);
        self.newline();
    }

    fn write_file(&mut self) {
        const PREFIX_LEN: usize = 6;
        let arguments = &self.command[PREFIX_LEN..self.command_len];
        let Some(separator) = arguments.iter().position(|byte| *byte == b' ') else {
            self.write_line("Usage: write NAME text");
            return;
        };
        let name = unsafe { core::str::from_utf8_unchecked(&arguments[..separator]) };
        let contents = &arguments[separator + 1..];
        let mut disk = crate::drivers::ata_pio::AtaPio::primary_master();
        let result = disk
            .identify()
            .map_err(crate::fs::DiskFsError::from)
            .and_then(|info| crate::fs::DiskFileSystem::mount(&mut disk, info.sectors as u64));

        match result {
            Ok(mut filesystem) => match filesystem.write_file(&mut disk, name, contents) {
                Ok(_) => self.write_line("File saved to disk."),
                Err(crate::fs::DiskFsError::FileAlreadyExists) => self.write_line("File already exists."),
                Err(crate::fs::DiskFsError::NoDirectorySpace) => self.write_line("Directory is full."),
                Err(crate::fs::DiskFsError::NoDataSpace) => self.write_line("Disk data area is full."),
                Err(crate::fs::DiskFsError::NameTooLong) => self.write_line("File name is invalid."),
                Err(crate::fs::DiskFsError::FileTooLarge) => self.write_line("File text is too large."),
                Err(_) => self.write_line("File save failed."),
            },
            Err(_) => self.write_line("Filesystem mount failed."),
        }
    }

    fn remove_file(&mut self) {
        const PREFIX_LEN: usize = 3;
        let name = unsafe {
            core::str::from_utf8_unchecked(&self.command[PREFIX_LEN..self.command_len])
        }
        .trim();
        if name.is_empty() {
            self.write_line("Usage: rm NAME");
            return;
        }
        let mut disk = crate::drivers::ata_pio::AtaPio::primary_master();
        let result = disk
            .identify()
            .map_err(crate::fs::DiskFsError::from)
            .and_then(|info| crate::fs::DiskFileSystem::mount(&mut disk, info.sectors as u64));

        match result {
            Ok(mut filesystem) => match filesystem.remove_file(&mut disk, name) {
                Ok(()) => self.write_line("File removed from disk."),
                Err(crate::fs::DiskFsError::NotFound) => self.write_line("File not found."),
                Err(crate::fs::DiskFsError::ProtectedFile) => self.write_line("Built-in file is read-only."),
                Err(_) => self.write_line("File remove failed."),
            },
            Err(_) => self.write_line("Filesystem mount failed."),
        }
    }

    fn show_filesystem_info(&mut self) {
        let mut disk = crate::drivers::ata_pio::AtaPio::primary_master();
        let result = disk
            .identify()
            .map_err(crate::fs::DiskFsError::from)
            .and_then(|info| crate::fs::DiskFileSystem::mount(&mut disk, info.sectors as u64));
        match result {
            Ok(filesystem) => {
                self.write_line("HAVA filesystem:");
                self.write_decimal("  files: ", filesystem.file_count() as u64);
                self.write_decimal("  free directory slots: ", filesystem.free_directory_slots() as u64);
                self.write_decimal("  free data sectors: ", filesystem.free_data_sectors() as u64);
            }
            Err(_) => self.write_line("Filesystem mount failed."),
        }
    }

    fn show_tasks(&mut self) {
        let (tasks, count, switches) = crate::task::snapshot();
        self.write_line("Scheduler tasks:");
        for task in tasks[..count].iter().flatten() {
            self.write_str("  ");
            self.write_decimal_inline(task.id as u64);
            self.write_str(": ");
            self.write_str(task.name);
            self.write_str(" (");
            self.write_str(task.state.label());
            self.write_str(", stack: ");
            match task.stack_bytes {
                Some(bytes) => {
                    self.write_decimal_inline((bytes / 1024) as u64);
                    self.write_str(" KiB");
                }
                None => self.write_str("boot"),
            }
            self.write_line(")");
        }
        self.write_str("Context switches planned: ");
        self.write_decimal_inline(switches);
        self.newline();
    }

    fn sleep_console(&mut self) {
        const PREFIX_LEN: usize = 6;
        let seconds = unsafe {
            core::str::from_utf8_unchecked(&self.command[PREFIX_LEN..self.command_len])
        }
        .trim()
        .parse::<u64>();
        match seconds {
            Ok(seconds) => {
                crate::task::sleep_console_for(seconds.saturating_mul(100));
                self.write_str("Console sleeping for ");
                self.write_decimal_inline(seconds);
                self.write_line(" seconds.");
            }
            Err(_) => self.write_line("Usage: sleep SECONDS"),
        }
    }

    fn push(&mut self, character: char) {
        if self.command_len < COMMAND_CAPACITY {
            self.command[self.command_len] = character as u8;
            self.command_len += 1;
            self.write_char(character);
        }
    }

    fn backspace(&mut self) {
        if self.command_len > 0 && self.column > 2 {
            self.command_len -= 1;
            self.column -= 1;
            self.erase_cell(self.column, self.row);
        }
    }

    fn prompt(&mut self) {
        self.write_str("> ");
    }

    fn clear(&mut self) {
        framebuffer::clear(self.framebuffer, BACKGROUND);
        self.column = 0;
        self.row = 0;
    }

    fn write_line(&mut self, text: &str) {
        self.write_str(text);
        self.newline();
    }

    fn write_str(&mut self, text: &str) {
        for character in text.chars() {
            self.write_char(character);
        }
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if byte.is_ascii_graphic() || *byte == b' ' {
                self.write_char(*byte as char);
            }
        }
    }

    fn write_decimal(&mut self, prefix: &str, value: u64) {
        self.write_str(prefix);
        self.write_decimal_inline(value);
        self.newline();
    }

    fn write_decimal_inline(&mut self, mut value: u64) {
        let mut digits = [0_u8; 20];
        let mut count = 0;
        loop {
            digits[count] = (value % 10) as u8;
            count += 1;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        for digit in digits[..count].iter().rev() {
            self.write_char((b'0' + *digit) as char);
        }
    }

    fn write_char(&mut self, character: char) {
        if character == '\n' {
            self.newline();
            return;
        }
        if self.column >= self.columns {
            self.newline();
        }
        framebuffer::draw_char(
            self.framebuffer,
            self.column * CELL_WIDTH,
            self.row * CELL_HEIGHT,
            character,
            SCALE,
            FOREGROUND,
        );
        self.column += 1;
    }

    fn newline(&mut self) {
        self.column = 0;
        self.row += 1;
        if self.row >= self.rows {
            framebuffer::scroll_up(self.framebuffer, self.rows, CELL_HEIGHT, BACKGROUND);
            self.row = self.rows - 1;
        }
    }

    fn erase_cell(&mut self, column: usize, row: usize) {
        framebuffer::fill_rect(
            self.framebuffer,
            column * CELL_WIDTH,
            row * CELL_HEIGHT,
            CELL_WIDTH,
            CELL_HEIGHT,
            BACKGROUND,
        );
    }
}
