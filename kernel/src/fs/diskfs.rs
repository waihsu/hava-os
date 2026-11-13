//! Read-only HAVA filesystem stored at the end of the boot disk.

use crate::drivers::block::{BlockDevice, BlockDeviceError, SECTOR_SIZE};

const MAGIC: &[u8; 8] = b"HAVAFS01";
const MAX_FILES: usize = 14;
const NAME_BYTES: usize = 24;
const DIRECTORY_OFFSET: usize = 32;
const DIRECTORY_ENTRY_SIZE: usize = 32;
const SEARCH_SECTORS: u64 = 64;

#[derive(Clone, Copy)]
pub struct DiskFile {
    name: [u8; NAME_BYTES],
    name_len: usize,
    start_sector: u64,
    length: usize,
}

impl DiskFile {
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    pub fn length(&self) -> usize {
        self.length
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DiskFsError {
    Device,
    NotFound,
    InvalidFormat,
    FileTooLarge,
    FileAlreadyExists,
    NameTooLong,
    NoDirectorySpace,
    NoDataSpace,
    ProtectedFile,
}

impl From<BlockDeviceError> for DiskFsError {
    fn from(_: BlockDeviceError) -> Self {
        Self::Device
    }
}

pub struct DiskFileSystem {
    files: [Option<DiskFile>; MAX_FILES],
    entry_count: usize,
    header_sector: u64,
    writable_start_sector: u32,
    disk_sectors: u64,
}

impl DiskFileSystem {
    /// Finds and mounts the HAVA filesystem header near the end of a disk.
    pub fn mount(
        device: &mut impl BlockDevice,
        disk_sectors: u64,
    ) -> Result<Self, DiskFsError> {
        let mut sector = [0_u8; SECTOR_SIZE];
        let first_sector = disk_sectors.saturating_sub(SEARCH_SECTORS);

        for header_sector in (first_sector..disk_sectors).rev() {
            device.read_sector(header_sector, &mut sector)?;
            if &sector[..MAGIC.len()] != MAGIC {
                continue;
            }
            if sector[8] != 1 || usize::from(sector[9]) > MAX_FILES {
                return Err(DiskFsError::InvalidFormat);
            }

            let entry_count = usize::from(sector[9]);
            let writable_start_sector = u32::from_le_bytes(sector[14..18].try_into().unwrap());
            let mut files = [None; MAX_FILES];
            for (index, slot) in files.iter_mut().take(entry_count).enumerate() {
                let entry = DIRECTORY_OFFSET + index * DIRECTORY_ENTRY_SIZE;
                let name_len = usize::from(sector[entry]);
                if name_len == 0 {
                    continue;
                }
                if name_len > NAME_BYTES {
                    return Err(DiskFsError::InvalidFormat);
                }
                let mut name = [0_u8; NAME_BYTES];
                name.copy_from_slice(&sector[entry + 1..entry + 25]);
                let relative_sector = u32::from_le_bytes(
                    sector[entry + 25..entry + 29].try_into().unwrap(),
                );
                let length = usize::from(u16::from_le_bytes(
                    sector[entry + 29..entry + 31].try_into().unwrap(),
                ));
                let file_sectors = length.div_ceil(SECTOR_SIZE) as u64;
                if header_sector + u64::from(relative_sector) + file_sectors > disk_sectors {
                    return Err(DiskFsError::InvalidFormat);
                }
                *slot = Some(DiskFile {
                    name,
                    name_len,
                    start_sector: header_sector + u64::from(relative_sector),
                    length,
                });
            }
            return Ok(Self {
                files,
                entry_count,
                header_sector,
                writable_start_sector,
                disk_sectors,
            });
        }
        Err(DiskFsError::NotFound)
    }

    pub fn files(&self) -> impl Iterator<Item = &DiskFile> {
        self.files[..self.entry_count].iter().flatten()
    }

    pub fn file_count(&self) -> usize {
        self.files().count()
    }

    /// Number of remaining directory entries, including reusable holes.
    pub fn free_directory_slots(&self) -> usize {
        MAX_FILES - self.file_count()
    }

    /// Number of free one-sector blocks in the writable file-data area.
    pub fn free_data_sectors(&self) -> usize {
        let Some(final_scratch_sector) = self.disk_sectors.checked_sub(1) else {
            return 0;
        };
        let Some(end_relative) = final_scratch_sector.checked_sub(self.header_sector) else {
            return 0;
        };
        let capacity = end_relative.saturating_sub(u64::from(self.writable_start_sector)) as usize;
        let used = self.files().filter(|file| {
            file.start_sector >= self.header_sector + u64::from(self.writable_start_sector)
        }).count();
        capacity.saturating_sub(used)
    }

    pub fn find(&self, name: &str) -> Option<&DiskFile> {
        self.files().find(|file| file.name() == name.as_bytes())
    }

    pub fn read(
        &self,
        device: &mut impl BlockDevice,
        name: &str,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<usize, DiskFsError> {
        let file = self.find(name).ok_or(DiskFsError::NotFound)?;
        if file.length > SECTOR_SIZE {
            return Err(DiskFsError::FileTooLarge);
        }
        self.read_file_sector(device, file, 0, destination)?;
        Ok(file.length)
    }

    /// Reads one sector from a file; callers can stream large files using a
    /// single reusable sector buffer.
    pub fn read_file_sector(
        &self,
        device: &mut impl BlockDevice,
        file: &DiskFile,
        file_sector: usize,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), DiskFsError> {
        if file_sector >= file.length.div_ceil(SECTOR_SIZE) {
            return Err(DiskFsError::NotFound);
        }
        device.read_sector(file.start_sector + file_sector as u64, destination)?;
        Ok(())
    }

    /// Creates a one-sector file in the reserved writable disk area.
    ///
    /// Empty sectors left by `remove_file` are selected before consuming new
    /// space, so deleting a user file makes its storage reusable.
    pub fn write_file(
        &mut self,
        device: &mut impl BlockDevice,
        name: &str,
        contents: &[u8],
    ) -> Result<u64, DiskFsError> {
        if name.is_empty() || name.len() > NAME_BYTES {
            return Err(DiskFsError::NameTooLong);
        }
        if contents.len() > SECTOR_SIZE {
            return Err(DiskFsError::FileTooLarge);
        }
        if self.find(name).is_some() {
            return Err(DiskFsError::FileAlreadyExists);
        }
        let entry_index = self.files[..self.entry_count]
            .iter()
            .position(Option::is_none)
            .or_else(|| (self.entry_count < MAX_FILES).then_some(self.entry_count))
            .ok_or(DiskFsError::NoDirectorySpace)?;
        let relative_sector = self.find_free_data_sector().ok_or(DiskFsError::NoDataSpace)?;
        let data_sector = self.header_sector + u64::from(relative_sector);

        let mut data = [0_u8; SECTOR_SIZE];
        data[..contents.len()].copy_from_slice(contents);
        device.write_sector(data_sector, &data)?;

        let mut header = [0_u8; SECTOR_SIZE];
        device.read_sector(self.header_sector, &mut header)?;
        let entry = DIRECTORY_OFFSET + entry_index * DIRECTORY_ENTRY_SIZE;
        header[entry..entry + DIRECTORY_ENTRY_SIZE].fill(0);
        header[entry] = name.len() as u8;
        header[entry + 1..entry + 1 + name.len()].copy_from_slice(name.as_bytes());
        header[entry + 25..entry + 29].copy_from_slice(&relative_sector.to_le_bytes());
        header[entry + 29..entry + 31].copy_from_slice(&(contents.len() as u16).to_le_bytes());
        self.entry_count = self.entry_count.max(entry_index + 1);
        header[9] = self.entry_count as u8;
        device.write_sector(self.header_sector, &header)?;

        let mut stored_name = [0_u8; NAME_BYTES];
        stored_name[..name.len()].copy_from_slice(name.as_bytes());
        self.files[entry_index] = Some(DiskFile {
            name: stored_name,
            name_len: name.len(),
            start_sector: data_sector,
            length: contents.len(),
        });
        Ok(data_sector)
    }

    /// Removes a directory entry. Its old data sector is not reclaimed yet.
    pub fn remove_file(&mut self, device: &mut impl BlockDevice, name: &str) -> Result<(), DiskFsError> {
        let entry_index = self.files[..self.entry_count]
            .iter()
            .position(|file| file.is_some_and(|file| file.name() == name.as_bytes()))
            .ok_or(DiskFsError::NotFound)?;
        if self.files[entry_index]
            .expect("found file must exist")
            .start_sector
            < self.header_sector + u64::from(self.writable_start_sector)
        {
            return Err(DiskFsError::ProtectedFile);
        }
        let mut header = [0_u8; SECTOR_SIZE];
        device.read_sector(self.header_sector, &mut header)?;
        let entry = DIRECTORY_OFFSET + entry_index * DIRECTORY_ENTRY_SIZE;
        header[entry..entry + DIRECTORY_ENTRY_SIZE].fill(0);
        self.files[entry_index] = None;
        while self.entry_count > 0 && self.files[self.entry_count - 1].is_none() {
            self.entry_count -= 1;
        }
        header[9] = self.entry_count as u8;
        device.write_sector(self.header_sector, &header)?;
        Ok(())
    }

    fn find_free_data_sector(&self) -> Option<u32> {
        let final_scratch_sector = self.disk_sectors.checked_sub(1)?;
        let end_relative = u32::try_from(final_scratch_sector.checked_sub(self.header_sector)?).ok()?;
        (self.writable_start_sector..end_relative).find(|relative_sector| {
            let sector = self.header_sector + u64::from(*relative_sector);
            !self.files().any(|file| file.start_sector == sector)
        })
    }
}
