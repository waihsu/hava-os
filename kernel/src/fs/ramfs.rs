//! Read-only files compiled into the kernel image.

use crate::fs::{File, FileSystem};

pub struct RamFileSystem;

const FILES: &[File] = &[
    File {
        name: "README",
        contents: "Welcome to HAVA OS. This is the in-memory filesystem.",
    },
    File {
        name: "LICENSE",
        contents: "HAVA OS is a learning kernel project.",
    },
];

impl FileSystem for RamFileSystem {
    fn files(&self) -> &'static [File] {
        FILES
    }
}
