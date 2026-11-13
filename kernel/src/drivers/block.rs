//! Common interface for sector-addressed storage devices.
//!
//! A future ATA, AHCI, or virtio-blk driver will implement this trait. The
//! filesystem will only depend on this interface, never on a specific disk
//! controller.

pub const SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockDeviceError {
    /// The requested sector lies outside the device's capacity.
    AddressOutOfRange,
    /// The controller reported an I/O failure.
    DeviceFault,
}

/// Reads and writes fixed-size sectors on a block-addressed storage device.
pub trait BlockDevice {
    fn read_sector(
        &mut self,
        sector: u64,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), BlockDeviceError>;

    fn write_sector(
        &mut self,
        sector: u64,
        source: &[u8; SECTOR_SIZE],
    ) -> Result<(), BlockDeviceError>;
}
