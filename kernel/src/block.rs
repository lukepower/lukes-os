use alloc::vec::Vec;

#[derive(Debug)]
pub enum BlockError {
    ReadError,
    WriteError,
    InvalidOffset,
    IoError,
}

pub type Result<T> = core::result::Result<T, BlockError>;

/// Unified Block Device Trait (storage-agnostic)
pub trait BlockDevice: Send + Sync {
    /// Read a block from the device.
    /// `lba`: Logical Block Address (sector index).
    /// `buf`: Buffer to read into. Must be block_size aligned and sized.
    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<()>;

    /// Write a block to the device.
    fn write_block(&self, lba: u64, buf: &[u8]) -> Result<()>;

    /// Block size in bytes (typically 512).
    fn block_size(&self) -> usize;

    /// Total number of blocks on the device.
    fn block_count(&self) -> u64;
}
