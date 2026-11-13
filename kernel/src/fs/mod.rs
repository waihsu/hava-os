//! Small filesystem interface used by the kernel console.
//!
//! The first implementation is an in-memory, read-only filesystem. Keeping
//! this trait separate lets a future disk-backed filesystem replace it without
//! changing the console.

mod diskfs;
mod ramfs;

pub use diskfs::{DiskFileSystem, DiskFsError};
pub use ramfs::RamFileSystem;

#[derive(Clone, Copy)]
pub struct File {
    pub name: &'static str,
    pub contents: &'static str,
}

/// Read-only operations required by the kernel's first filesystem layer.
pub trait FileSystem {
    fn files(&self) -> &'static [File];

    fn read(&self, name: &str) -> Option<&'static str> {
        self.files()
            .iter()
            .find(|file| file.name == name)
            .map(|file| file.contents)
    }
}

/// The filesystem mounted at the root of HAVA OS during early development.
pub static ROOT: RamFileSystem = RamFileSystem;
