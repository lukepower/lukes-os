use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::RwLock;

use crate::block::BlockDevice;
use crate::vfs::{FileSystem, FileType, Inode, Metadata, Result, VfsError};

const SECTOR_SIZE: usize = 512;
const MAGIC: &[u8; 8] = b"LUKEFS01";
const MAX_NAME_LEN: usize = 28;
const MAX_ENTRIES: usize = 126;
const TABLE_SECTORS: u64 = 8;
const DATA_START_LBA: u64 = 1 + TABLE_SECTORS; // Sector 9

#[derive(Clone, Copy)]
#[repr(C)]
struct OnDiskEntry {
    name: [u8; MAX_NAME_LEN],
    is_dir: u8,
    _pad: u8,
    size: u16,
    start_sector: u16,
    sector_count: u16,
}

#[derive(Clone)]
struct FileRecord {
    is_dir: bool,
    size: u64,
    start_sector: u16,
    sector_count: u16,
}

struct LukeFsState {
    entries: BTreeMap<String, FileRecord>,
    next_free_sector: u16,
}

pub struct LukeFs {
    device: Arc<dyn BlockDevice>,
    state: Arc<RwLock<LukeFsState>>,
}

impl LukeFs {
    /// Mount or format a `LukeFs` on the given block device.
    pub fn mount(device: Arc<dyn BlockDevice>) -> Result<Arc<Self>> {
        let mut magic_buf = [0u8; SECTOR_SIZE];
        device.read_block(0, &mut magic_buf).map_err(|_| VfsError::IoError)?;

        let state = if &magic_buf[0..8] == MAGIC {
            Self::read_table(&device)?
        } else {
            let init_state = LukeFsState {
                entries: BTreeMap::new(),
                next_free_sector: 0,
            };
            Self::write_table(&device, &init_state)?;
            init_state
        };

        Ok(Arc::new(LukeFs {
            device,
            state: Arc::new(RwLock::new(state)),
        }))
    }

    fn read_table(device: &Arc<dyn BlockDevice>) -> Result<LukeFsState> {
        let mut table_buf = alloc::vec![0u8; (TABLE_SECTORS as usize) * SECTOR_SIZE];
        for s in 0..TABLE_SECTORS {
            let offset = (s as usize) * SECTOR_SIZE;
            device
                .read_block(1 + s, &mut table_buf[offset..offset + SECTOR_SIZE])
                .map_err(|_| VfsError::IoError)?;
        }

        let num_entries = u32::from_le_bytes(table_buf[0..4].try_into().unwrap()) as usize;
        let next_free = u16::from_le_bytes(table_buf[4..6].try_into().unwrap());

        let mut entries = BTreeMap::new();
        let mut cursor = 8;
        let entry_size = core::mem::size_of::<OnDiskEntry>();

        for _ in 0..num_entries.min(MAX_ENTRIES) {
            let slice = &table_buf[cursor..cursor + entry_size];
            cursor += entry_size;

            let name_bytes = &slice[0..MAX_NAME_LEN];
            let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(MAX_NAME_LEN);
            let name = core::str::from_utf8(&name_bytes[..name_len])
                .map_err(|_| VfsError::IoError)?
                .to_string();

            let is_dir = slice[MAX_NAME_LEN] != 0;
            let size = u16::from_le_bytes(slice[30..32].try_into().unwrap()) as u64;
            let start_sector = u16::from_le_bytes(slice[32..34].try_into().unwrap());
            let sector_count = u16::from_le_bytes(slice[34..36].try_into().unwrap());

            if !name.is_empty() {
                entries.insert(
                    name,
                    FileRecord {
                        is_dir,
                        size,
                        start_sector,
                        sector_count,
                    },
                );
            }
        }

        Ok(LukeFsState {
            entries,
            next_free_sector: next_free,
        })
    }

    fn write_table(device: &Arc<dyn BlockDevice>, state: &LukeFsState) -> Result<()> {
        let mut sector0 = [0u8; SECTOR_SIZE];
        sector0[0..8].copy_from_slice(MAGIC);
        device.write_block(0, &sector0).map_err(|_| VfsError::IoError)?;

        let mut table_buf = alloc::vec![0u8; (TABLE_SECTORS as usize) * SECTOR_SIZE];
        let num_entries = state.entries.len() as u32;
        table_buf[0..4].copy_from_slice(&num_entries.to_le_bytes());
        table_buf[4..6].copy_from_slice(&state.next_free_sector.to_le_bytes());

        let mut cursor = 8;
        let entry_size = core::mem::size_of::<OnDiskEntry>();

        for (name, record) in &state.entries {
            if cursor + entry_size > table_buf.len() {
                break;
            }
            let slice = &mut table_buf[cursor..cursor + entry_size];
            cursor += entry_size;

            let name_bytes = name.as_bytes();
            let copy_len = name_bytes.len().min(MAX_NAME_LEN);
            slice[0..copy_len].copy_from_slice(&name_bytes[..copy_len]);

            slice[MAX_NAME_LEN] = if record.is_dir { 1 } else { 0 };
            slice[30..32].copy_from_slice(&(record.size as u16).to_le_bytes());
            slice[32..34].copy_from_slice(&record.start_sector.to_le_bytes());
            slice[34..36].copy_from_slice(&record.sector_count.to_le_bytes());
        }

        for s in 0..TABLE_SECTORS {
            let offset = (s as usize) * SECTOR_SIZE;
            device
                .write_block(1 + s, &table_buf[offset..offset + SECTOR_SIZE])
                .map_err(|_| VfsError::IoError)?;
        }

        Ok(())
    }
}

impl FileSystem for LukeFs {
    fn fs_type(&self) -> &'static str {
        "lukefs"
    }

    fn root(&self) -> Arc<dyn Inode> {
        Arc::new(LukeFsNode {
            name: String::new(),
            device: self.device.clone(),
            state: self.state.clone(),
            is_root: true,
            is_dir: true,
        })
    }

    fn sync(&self) -> Result<()> {
        let guard = self.state.read();
        LukeFs::write_table(&self.device, &guard)
    }
}

pub struct LukeFsNode {
    name: String,
    device: Arc<dyn BlockDevice>,
    state: Arc<RwLock<LukeFsState>>,
    is_root: bool,
    is_dir: bool,
}

impl Inode for LukeFsNode {
    fn metadata(&self) -> Result<Metadata> {
        if self.is_root || self.is_dir {
            Ok(Metadata {
                file_type: FileType::Directory,
                size: 0,
            })
        } else {
            let guard = self.state.read();
            let record = guard.entries.get(&self.name).ok_or(VfsError::NotFound)?;
            Ok(Metadata {
                file_type: FileType::File,
                size: record.size,
            })
        }
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        if self.is_dir {
            return Err(VfsError::IsADirectory);
        }

        let guard = self.state.read();
        let record = guard.entries.get(&self.name).ok_or(VfsError::NotFound)?;
        if offset >= record.size {
            return Ok(0);
        }

        let to_read = core::cmp::min(buf.len() as u64, record.size - offset) as usize;
        let mut read_bytes = 0;
        let mut sector_scratch = [0u8; SECTOR_SIZE];

        while read_bytes < to_read {
            let current_file_offset = offset + (read_bytes as u64);
            let sector_in_file = (current_file_offset / (SECTOR_SIZE as u64)) as u16;
            let offset_in_sector = (current_file_offset % (SECTOR_SIZE as u64)) as usize;

            let abs_lba = DATA_START_LBA + (record.start_sector as u64) + (sector_in_file as u64);
            self.device
                .read_block(abs_lba, &mut sector_scratch)
                .map_err(|_| VfsError::IoError)?;

            let available_in_sector = SECTOR_SIZE - offset_in_sector;
            let chunk_len = core::cmp::min(to_read - read_bytes, available_in_sector);

            buf[read_bytes..read_bytes + chunk_len]
                .copy_from_slice(&sector_scratch[offset_in_sector..offset_in_sector + chunk_len]);
            read_bytes += chunk_len;
        }

        Ok(read_bytes)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize> {
        if self.is_dir {
            return Err(VfsError::IsADirectory);
        }

        let mut guard = self.state.write();
        let mut record = guard.entries.get(&self.name).cloned().ok_or(VfsError::NotFound)?;

        let end_offset = offset + (buf.len() as u64);
        let needed_sectors = ((end_offset + (SECTOR_SIZE as u64) - 1) / (SECTOR_SIZE as u64)) as u16;

        // Ensure sufficient sectors allocated
        if needed_sectors > record.sector_count {
            let additional = needed_sectors - record.sector_count;
            if (guard.next_free_sector as u64) + (additional as u64) + DATA_START_LBA > self.device.block_count() {
                return Err(VfsError::NoSpace);
            }
            // For simple linear allocation: if file is already at top, expand it; otherwise relocate
            if record.start_sector + record.sector_count == guard.next_free_sector {
                guard.next_free_sector += additional;
                record.sector_count = needed_sectors;
            } else {
                // Relocate to next free sector
                let new_start = guard.next_free_sector;
                guard.next_free_sector += needed_sectors;

                // Copy existing sectors
                let mut move_buf = [0u8; SECTOR_SIZE];
                for s in 0..record.sector_count {
                    let old_lba = DATA_START_LBA + (record.start_sector as u64) + (s as u64);
                    let new_lba = DATA_START_LBA + (new_start as u64) + (s as u64);
                    self.device.read_block(old_lba, &mut move_buf).map_err(|_| VfsError::IoError)?;
                    self.device.write_block(new_lba, &move_buf).map_err(|_| VfsError::IoError)?;
                }

                record.start_sector = new_start;
                record.sector_count = needed_sectors;
            }
        }

        // Perform write into allocated sectors
        let mut written = 0;
        let mut sector_scratch = [0u8; SECTOR_SIZE];

        while written < buf.len() {
            let current_file_offset = offset + (written as u64);
            let sector_in_file = (current_file_offset / (SECTOR_SIZE as u64)) as u16;
            let offset_in_sector = (current_file_offset % (SECTOR_SIZE as u64)) as usize;

            let abs_lba = DATA_START_LBA + (record.start_sector as u64) + (sector_in_file as u64);

            // Read existing sector if partial write
            if offset_in_sector > 0 || (buf.len() - written) < SECTOR_SIZE {
                let _ = self.device.read_block(abs_lba, &mut sector_scratch);
            }

            let available_in_sector = SECTOR_SIZE - offset_in_sector;
            let chunk_len = core::cmp::min(buf.len() - written, available_in_sector);

            sector_scratch[offset_in_sector..offset_in_sector + chunk_len]
                .copy_from_slice(&buf[written..written + chunk_len]);

            self.device
                .write_block(abs_lba, &sector_scratch)
                .map_err(|_| VfsError::IoError)?;

            written += chunk_len;
        }

        if end_offset > record.size {
            record.size = end_offset;
        }

        guard.entries.insert(self.name.clone(), record);
        LukeFs::write_table(&self.device, &guard)?;

        Ok(buf.len())
    }

    fn truncate(&self, size: u64) -> Result<()> {
        if self.is_dir {
            return Err(VfsError::IsADirectory);
        }

        let mut guard = self.state.write();
        let mut record = guard.entries.get(&self.name).cloned().ok_or(VfsError::NotFound)?;
        record.size = size;
        guard.entries.insert(self.name.clone(), record);
        LukeFs::write_table(&self.device, &guard)
    }

    fn lookup(&self, name: &str) -> Result<Arc<dyn Inode>> {
        if !self.is_dir {
            return Err(VfsError::NotADirectory);
        }

        let guard = self.state.read();
        let record = guard.entries.get(name).ok_or(VfsError::NotFound)?;
        Ok(Arc::new(LukeFsNode {
            name: name.to_string(),
            device: self.device.clone(),
            state: self.state.clone(),
            is_root: false,
            is_dir: record.is_dir,
        }))
    }

    fn create(&self, name: &str) -> Result<Arc<dyn Inode>> {
        if !self.is_dir {
            return Err(VfsError::NotADirectory);
        }
        if name.as_bytes().len() > MAX_NAME_LEN {
            return Err(VfsError::InvalidInput);
        }

        let mut guard = self.state.write();
        if guard.entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        if guard.entries.len() >= MAX_ENTRIES {
            return Err(VfsError::NoSpace);
        }

        let start_sector = guard.next_free_sector;
        guard.next_free_sector += 1;

        guard.entries.insert(
            name.to_string(),
            FileRecord {
                is_dir: false,
                size: 0,
                start_sector,
                sector_count: 1,
            },
        );

        LukeFs::write_table(&self.device, &guard)?;

        Ok(Arc::new(LukeFsNode {
            name: name.to_string(),
            device: self.device.clone(),
            state: self.state.clone(),
            is_root: false,
            is_dir: false,
        }))
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn Inode>> {
        if !self.is_dir {
            return Err(VfsError::NotADirectory);
        }
        if name.as_bytes().len() > MAX_NAME_LEN {
            return Err(VfsError::InvalidInput);
        }

        let mut guard = self.state.write();
        if guard.entries.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        if guard.entries.len() >= MAX_ENTRIES {
            return Err(VfsError::NoSpace);
        }

        guard.entries.insert(
            name.to_string(),
            FileRecord {
                is_dir: true,
                size: 0,
                start_sector: 0,
                sector_count: 0,
            },
        );

        LukeFs::write_table(&self.device, &guard)?;

        Ok(Arc::new(LukeFsNode {
            name: name.to_string(),
            device: self.device.clone(),
            state: self.state.clone(),
            is_root: false,
            is_dir: true,
        }))
    }

    fn unlink(&self, name: &str) -> Result<()> {
        if !self.is_dir {
            return Err(VfsError::NotADirectory);
        }

        let mut guard = self.state.write();
        guard.entries.remove(name).ok_or(VfsError::NotFound)?;
        LukeFs::write_table(&self.device, &guard)
    }

    fn readdir(&self) -> Result<Vec<String>> {
        if !self.is_dir {
            return Err(VfsError::NotADirectory);
        }

        let guard = self.state.read();
        Ok(guard.entries.keys().cloned().collect())
    }

    fn sync(&self) -> Result<()> {
        let guard = self.state.read();
        LukeFs::write_table(&self.device, &guard)
    }
}
