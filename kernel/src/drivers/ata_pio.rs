//! Primary IDE controller driver using ATA programmed I/O (PIO).
//!
//! This is intentionally small: it reads one 512-byte LBA28 sector at a time.
//! It is enough to prove the block-device boundary before adding a filesystem
//! parser or a faster DMA-based controller.

use x86_64::instructions::port::Port;

use crate::drivers::block::{BlockDevice, BlockDeviceError, SECTOR_SIZE};

const ATA_DATA: u16 = 0x1f0;
const ATA_SECTOR_COUNT: u16 = 0x1f2;
const ATA_LBA_LOW: u16 = 0x1f3;
const ATA_LBA_MID: u16 = 0x1f4;
const ATA_LBA_HIGH: u16 = 0x1f5;
const ATA_DRIVE: u16 = 0x1f6;
const ATA_STATUS: u16 = 0x1f7;
const ATA_COMMAND_READ_SECTORS: u8 = 0x20;
const ATA_COMMAND_WRITE_SECTORS: u8 = 0x30;
const ATA_COMMAND_IDENTIFY_DEVICE: u8 = 0xec;
const STATUS_BUSY: u8 = 0x80;
const STATUS_DRQ: u8 = 0x08;
const STATUS_ERROR: u8 = 0x01;
const STATUS_DEVICE_FAULT: u8 = 0x20;
const POLL_LIMIT: usize = 1_000_000;

pub struct AtaPio;

#[derive(Clone, Copy)]
pub struct DiskInfo {
    /// ATA model field, normalized to ordinary byte order.
    pub model: [u8; 40],
    /// Number of addressable 512-byte sectors reported through LBA28.
    pub sectors: u32,
}

impl AtaPio {
    /// Creates a driver for QEMU's primary IDE master disk.
    pub const fn primary_master() -> Self {
        Self
    }

    fn wait_for_data(&mut self) -> Result<(), BlockDeviceError> {
        let mut status = Port::<u8>::new(ATA_STATUS);
        for _ in 0..POLL_LIMIT {
            let value = unsafe { status.read() };
            if value == 0 || value & (STATUS_ERROR | STATUS_DEVICE_FAULT) != 0 {
                return Err(BlockDeviceError::DeviceFault);
            }
            if value & STATUS_BUSY == 0 && value & STATUS_DRQ != 0 {
                return Ok(());
            }
        }
        Err(BlockDeviceError::DeviceFault)
    }

    fn wait_not_busy(&mut self) -> Result<(), BlockDeviceError> {
        let mut status = Port::<u8>::new(ATA_STATUS);
        for _ in 0..POLL_LIMIT {
            let value = unsafe { status.read() };
            if value == 0 || value & (STATUS_ERROR | STATUS_DEVICE_FAULT) != 0 {
                return Err(BlockDeviceError::DeviceFault);
            }
            if value & STATUS_BUSY == 0 {
                return Ok(());
            }
        }
        Err(BlockDeviceError::DeviceFault)
    }

    /// Queries the primary IDE master using ATA IDENTIFY DEVICE.
    pub fn identify(&mut self) -> Result<DiskInfo, BlockDeviceError> {
        self.wait_not_busy()?;
        unsafe {
            Port::<u8>::new(ATA_DRIVE).write(0xe0);
            Port::<u8>::new(ATA_SECTOR_COUNT).write(0);
            Port::<u8>::new(ATA_LBA_LOW).write(0);
            Port::<u8>::new(ATA_LBA_MID).write(0);
            Port::<u8>::new(ATA_LBA_HIGH).write(0);
            Port::<u8>::new(ATA_STATUS).write(ATA_COMMAND_IDENTIFY_DEVICE);
        }
        self.wait_for_data()?;

        let mut words = [0_u16; 256];
        let mut data = Port::<u16>::new(ATA_DATA);
        for word in &mut words {
            *word = unsafe { data.read() };
        }

        let mut model = [b' '; 40];
        for (index, word) in words[27..47].iter().enumerate() {
            model[index * 2] = (word >> 8) as u8;
            model[index * 2 + 1] = *word as u8;
        }

        Ok(DiskInfo {
            model,
            sectors: u32::from(words[60]) | (u32::from(words[61]) << 16),
        })
    }
}

impl BlockDevice for AtaPio {
    fn read_sector(
        &mut self,
        sector: u64,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), BlockDeviceError> {
        if sector > 0x0fff_ffff {
            return Err(BlockDeviceError::AddressOutOfRange);
        }
        self.wait_not_busy()?;

        unsafe {
            Port::<u8>::new(ATA_DRIVE).write(0xe0 | ((sector >> 24) as u8 & 0x0f));
            Port::<u8>::new(ATA_SECTOR_COUNT).write(1);
            Port::<u8>::new(ATA_LBA_LOW).write(sector as u8);
            Port::<u8>::new(ATA_LBA_MID).write((sector >> 8) as u8);
            Port::<u8>::new(ATA_LBA_HIGH).write((sector >> 16) as u8);
            Port::<u8>::new(ATA_STATUS).write(ATA_COMMAND_READ_SECTORS);
        }
        self.wait_for_data()?;

        let mut data = Port::<u16>::new(ATA_DATA);
        for index in 0..(SECTOR_SIZE / 2) {
            let word = unsafe { data.read() };
            destination[index * 2] = word as u8;
            destination[index * 2 + 1] = (word >> 8) as u8;
        }
        Ok(())
    }

    fn write_sector(
        &mut self,
        sector: u64,
        source: &[u8; SECTOR_SIZE],
    ) -> Result<(), BlockDeviceError> {
        if sector > 0x0fff_ffff {
            return Err(BlockDeviceError::AddressOutOfRange);
        }
        self.wait_not_busy()?;

        unsafe {
            Port::<u8>::new(ATA_DRIVE).write(0xe0 | ((sector >> 24) as u8 & 0x0f));
            Port::<u8>::new(ATA_SECTOR_COUNT).write(1);
            Port::<u8>::new(ATA_LBA_LOW).write(sector as u8);
            Port::<u8>::new(ATA_LBA_MID).write((sector >> 8) as u8);
            Port::<u8>::new(ATA_LBA_HIGH).write((sector >> 16) as u8);
            Port::<u8>::new(ATA_STATUS).write(ATA_COMMAND_WRITE_SECTORS);
        }
        self.wait_for_data()?;

        let mut data = Port::<u16>::new(ATA_DATA);
        for index in 0..(SECTOR_SIZE / 2) {
            let word = u16::from(source[index * 2]) | (u16::from(source[index * 2 + 1]) << 8);
            unsafe { data.write(word) };
        }
        self.wait_not_busy()
    }
}
