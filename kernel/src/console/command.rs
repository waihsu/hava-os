#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,
    Memory,
    Uptime,
    Echo,
    ListFiles,
    Cat,
    Disk,
    DiskInfo,
    DiskWrite,
    Note,
    NoteWrite,
    Write,
    Remove,
    FileSystemInfo,
    Tasks,
    Sleep,
    Clear,
    Empty,
    Unknown,
}

pub fn parse(input: &str) -> Command {
    let trimmed = input.trim();
    if trimmed == "echo" || input.starts_with("echo ") {
        return Command::Echo;
    }
    if input.starts_with("cat ") {
        return Command::Cat;
    }
    if input.starts_with("note ") {
        return Command::NoteWrite;
    }
    if input.starts_with("write ") {
        return Command::Write;
    }
    if input.starts_with("rm ") {
        return Command::Remove;
    }
    if input.starts_with("sleep ") {
        return Command::Sleep;
    }

    match trimmed {
        "help" => Command::Help,
        "mem" => Command::Memory,
        "uptime" => Command::Uptime,
        "ls" => Command::ListFiles,
        "disk" => Command::Disk,
        "disk info" => Command::DiskInfo,
        "disk write" => Command::DiskWrite,
        "fs info" => Command::FileSystemInfo,
        "tasks" => Command::Tasks,
        "note" => Command::Note,
        "clear" => Command::Clear,
        "" => Command::Empty,
        _ => Command::Unknown,
    }
}
