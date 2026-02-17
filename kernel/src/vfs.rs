use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct Metadata {
    pub file_type: FileType,
    pub size: u64,
}

#[derive(Debug)]
pub enum VfsError {
    NotFound,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    InvalidInput,
    IoError,
}

pub type Result<T> = core::result::Result<T, VfsError>;

/// Trait for a filesystem inode (file or directory).
pub trait Inode: Send + Sync {
    fn metadata(&self) -> Result<Metadata>;

    /// Read data from the file at the given offset.
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        Err(VfsError::IsADirectory)
    }

    /// Write data to the file at the given offset.
    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        Err(VfsError::IsADirectory)
    }

    /// Look up a directory entry by name.
    fn lookup(&self, _name: &str) -> Result<Arc<dyn Inode>> {
        Err(VfsError::NotADirectory)
    }

    /// Create a new file in this directory.
    fn create(&self, _name: &str) -> Result<Arc<dyn Inode>> {
        Err(VfsError::NotADirectory)
    }

    /// Create a new directory in this directory.
    fn mkdir(&self, _name: &str) -> Result<Arc<dyn Inode>> {
        Err(VfsError::NotADirectory)
    }

    /// Read directory entries.
    fn readdir(&self) -> Result<Vec<String>> {
        Err(VfsError::NotADirectory)
    }
}

/// Abstract filesystem trait.
pub trait FileSystem: Send + Sync {
    fn root(&self) -> Arc<dyn Inode>;
}

/// Global mount point registry (simplified for now).
/// We'll just have a single root filesystem for Phase 5.1.
static ROOT_FS: RwLock<Option<Arc<dyn FileSystem>>> = RwLock::new(None);

pub fn mount_root(fs: Arc<dyn FileSystem>) {
    *ROOT_FS.write() = Some(fs);
}

pub fn root() -> Option<Arc<dyn Inode>> {
    ROOT_FS.read().as_ref().map(|fs| fs.root())
}
