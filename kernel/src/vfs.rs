use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekFrom {
    Start(u64),
    Current(i64),
    End(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    InvalidInput,
    IoError,
    NoSpace,
    ReadOnly,
    NotSupported,
    Busy,
}

pub type Result<T> = core::result::Result<T, VfsError>;

/// Flags controlling how a file is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub append: bool,
}

impl OpenFlags {
    pub const READ: Self = Self {
        read: true,
        write: false,
        create: false,
        truncate: false,
        append: false,
    };

    pub const WRITE: Self = Self {
        read: false,
        write: true,
        create: true,
        truncate: false,
        append: false,
    };

    pub const READ_WRITE: Self = Self {
        read: true,
        write: true,
        create: false,
        truncate: false,
        append: false,
    };

    pub const CREATE_OR_TRUNCATE: Self = Self {
        read: true,
        write: true,
        create: true,
        truncate: true,
        append: false,
    };
}

/// Core trait representing an individual inode (file or directory).
pub trait Inode: Send + Sync {
    fn metadata(&self) -> Result<Metadata>;

    /// Read data from file at offset.
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        Err(VfsError::IsADirectory)
    }

    /// Write data to file at offset.
    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        Err(VfsError::IsADirectory)
    }

    /// Truncate file to specified size.
    fn truncate(&self, _size: u64) -> Result<()> {
        Err(VfsError::IsADirectory)
    }

    /// Look up a child entry by name.
    fn lookup(&self, _name: &str) -> Result<Arc<dyn Inode>> {
        Err(VfsError::NotADirectory)
    }

    /// Create a regular file in this directory.
    fn create(&self, _name: &str) -> Result<Arc<dyn Inode>> {
        Err(VfsError::NotADirectory)
    }

    /// Create a subdirectory in this directory.
    fn mkdir(&self, _name: &str) -> Result<Arc<dyn Inode>> {
        Err(VfsError::NotADirectory)
    }

    /// Remove a child entry (file or empty directory).
    fn unlink(&self, _name: &str) -> Result<()> {
        Err(VfsError::NotADirectory)
    }

    /// Read directory entry names.
    fn readdir(&self) -> Result<Vec<String>> {
        Err(VfsError::NotADirectory)
    }

    /// Flush any cached changes to underlying storage.
    fn sync(&self) -> Result<()> {
        Ok(())
    }
}

/// Abstract filesystem trait.
pub trait FileSystem: Send + Sync {
    /// Returns the filesystem type identifier (e.g. "ramfs", "simplefs", "fat32").
    fn fs_type(&self) -> &'static str;

    /// Returns the root inode of this filesystem instance.
    fn root(&self) -> Arc<dyn Inode>;

    /// Flushes all cached filesystem metadata and blocks to underlying medium.
    fn sync(&self) -> Result<()> {
        Ok(())
    }
}

/// High-level open file descriptor with automatic cursor tracking.
pub struct FileHandle {
    inode: Arc<dyn Inode>,
    flags: OpenFlags,
    offset: u64,
}

impl FileHandle {
    pub fn new(inode: Arc<dyn Inode>, flags: OpenFlags) -> Self {
        Self {
            inode,
            flags,
            offset: 0,
        }
    }

    pub fn inode(&self) -> Arc<dyn Inode> {
        self.inode.clone()
    }

    pub fn metadata(&self) -> Result<Metadata> {
        self.inode.metadata()
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if !self.flags.read {
            return Err(VfsError::InvalidInput);
        }
        let read_bytes = self.inode.read(self.offset, buf)?;
        self.offset += read_bytes as u64;
        Ok(read_bytes)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if !self.flags.write {
            return Err(VfsError::ReadOnly);
        }
        if self.flags.append {
            let meta = self.inode.metadata()?;
            self.offset = meta.size;
        }
        let written = self.inode.write(self.offset, buf)?;
        self.offset += written as u64;
        Ok(written)
    }

    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let meta = self.inode.metadata()?;
        let new_offset = match pos {
            SeekFrom::Start(off) => off as i64,
            SeekFrom::Current(off) => (self.offset as i64) + off,
            SeekFrom::End(off) => (meta.size as i64) + off,
        };

        if new_offset < 0 {
            return Err(VfsError::InvalidInput);
        }

        self.offset = new_offset as u64;
        Ok(self.offset)
    }

    pub fn sync(&self) -> Result<()> {
        self.inode.sync()
    }
}

/// Global Mount Table supporting `/` and submounts (e.g. `/mnt`, `/disk`).
struct MountTable {
    root_fs: Option<Arc<dyn FileSystem>>,
    mounts: BTreeMap<String, Arc<dyn FileSystem>>,
}

static MOUNT_TABLE: RwLock<MountTable> = RwLock::new(MountTable {
    root_fs: None,
    mounts: BTreeMap::new(),
});

/// Mount a filesystem at `/` as the primary root.
pub fn mount_root(fs: Arc<dyn FileSystem>) {
    let mut table = MOUNT_TABLE.write();
    table.root_fs = Some(fs);
}

/// Mount a filesystem at a specific path (e.g. `"/mnt"` or `"/disk"`).
pub fn mount(target_path: &str, fs: Arc<dyn FileSystem>) -> Result<()> {
    let norm = normalize_path(target_path);
    if norm == "/" {
        mount_root(fs);
        return Ok(());
    }

    let mut table = MOUNT_TABLE.write();
    if table.mounts.contains_key(&norm) {
        return Err(VfsError::AlreadyExists);
    }
    table.mounts.insert(norm, fs);
    Ok(())
}

/// Unmount a filesystem by path.
pub fn unmount(target_path: &str) -> Result<()> {
    let norm = normalize_path(target_path);
    let mut table = MOUNT_TABLE.write();
    if norm == "/" {
        if table.root_fs.is_none() {
            return Err(VfsError::NotFound);
        }
        table.root_fs = None;
        return Ok(());
    }
    table.mounts.remove(&norm).map(|_| ()).ok_or(VfsError::NotFound)
}

/// Query the root inode of the primary filesystem.
pub fn root() -> Option<Arc<dyn Inode>> {
    let table = MOUNT_TABLE.read();
    table.root_fs.as_ref().map(|fs| fs.root())
}

/// Normalize path string (resolves `.` and `..`, removes redundant slashes).
pub fn normalize_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    let is_absolute = path.starts_with('/');

    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        } else if part == ".." {
            segments.pop();
        } else {
            segments.push(part);
        }
    }

    if segments.is_empty() {
        if is_absolute {
            return String::from("/");
        } else {
            return String::from(".");
        }
    }

    let mut result = String::new();
    if is_absolute {
        result.push('/');
    }
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            result.push('/');
        }
        result.push_str(seg);
    }
    result
}

/// Resolve a normalized path to its corresponding Inode, traversing mount points.
pub fn resolve_path(path: &str) -> Result<Arc<dyn Inode>> {
    let norm = normalize_path(path);
    let table = MOUNT_TABLE.read();

    // Check exact mount point matches first
    if let Some(fs) = table.mounts.get(&norm) {
        return Ok(fs.root());
    }

    // Check if path is within a mounted sub-filesystem
    // Find longest prefix mount point
    let mut best_mount: Option<(&String, &Arc<dyn FileSystem>)> = None;
    for (mount_path, fs) in table.mounts.iter() {
        if norm.starts_with(mount_path) {
            let next_char = norm.as_bytes().get(mount_path.len());
            if next_char.is_none() || next_char == Some(&b'/') {
                if best_mount.is_none() || mount_path.len() > best_mount.unwrap().0.len() {
                    best_mount = Some((mount_path, fs));
                }
            }
        }
    }

    let (mut current_node, sub_path) = if let Some((m_path, fs)) = best_mount {
        let sub = &norm[m_path.len()..];
        (fs.root(), sub.trim_start_matches('/'))
    } else {
        let root = table.root_fs.as_ref().ok_or(VfsError::NotFound)?.root();
        (root, norm.trim_start_matches('/'))
    };

    if sub_path.is_empty() {
        return Ok(current_node);
    }

    for segment in sub_path.split('/') {
        if segment.is_empty() {
            continue;
        }
        current_node = current_node.lookup(segment)?;
    }

    Ok(current_node)
}

/// Open an Inode by path with the specified OpenFlags.
pub fn open(path: &str, flags: OpenFlags) -> Result<FileHandle> {
    let norm = normalize_path(path);
    match resolve_path(&norm) {
        Ok(inode) => {
            let meta = inode.metadata()?;
            if meta.file_type == FileType::Directory && flags.write {
                return Err(VfsError::IsADirectory);
            }
            if flags.truncate && meta.file_type == FileType::File {
                inode.truncate(0)?;
            }
            Ok(FileHandle::new(inode, flags))
        }
        Err(VfsError::NotFound) if flags.create => {
            // Find parent directory and create file
            let (parent_path, filename) = split_parent(&norm)?;
            let parent_node = resolve_path(&parent_path)?;
            let new_file = parent_node.create(&filename)?;
            Ok(FileHandle::new(new_file, flags))
        }
        Err(e) => Err(e),
    }
}

/// Create a directory at the given path.
pub fn mkdir(path: &str) -> Result<Arc<dyn Inode>> {
    let norm = normalize_path(path);
    let (parent_path, dirname) = split_parent(&norm)?;
    let parent = resolve_path(&parent_path)?;
    parent.mkdir(&dirname)
}

/// Remove a file or empty directory at the given path.
pub fn unlink(path: &str) -> Result<()> {
    let norm = normalize_path(path);
    let (parent_path, name) = split_parent(&norm)?;
    let parent = resolve_path(&parent_path)?;
    parent.unlink(&name)
}

/// Read all directory entry names at the given path.
pub fn readdir(path: &str) -> Result<Vec<String>> {
    let node = resolve_path(path)?;
    node.readdir()
}

/// Helper function splitting a path into (parent_path, basename).
fn split_parent(norm_path: &str) -> Result<(String, String)> {
    let trimmed = norm_path.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(VfsError::InvalidInput);
    }

    if let Some(idx) = trimmed.rfind('/') {
        let parent = if idx == 0 {
            String::from("/")
        } else {
            String::from(&trimmed[..idx])
        };
        let base = String::from(&trimmed[idx + 1..]);
        Ok((parent, base))
    } else {
        Ok((String::from("/"), String::from(trimmed)))
    }
}
