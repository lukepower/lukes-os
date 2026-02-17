use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::RwLock;

use crate::vfs::{self, FileSystem, FileType, Inode, Metadata, Result, VfsError};

pub struct RamFs {
    root: Arc<RamNode>,
}

impl RamFs {
    pub fn new() -> Arc<Self> {
        Arc::new(RamFs {
            root: Arc::new(RamNode::new_dir()),
        })
    }
}

impl FileSystem for RamFs {
    fn root(&self) -> Arc<dyn Inode> {
        self.root.clone()
    }
}

pub struct RamNode {
    inner: RwLock<RamNodeInner>,
}

enum RamNodeInner {
    File { content: Vec<u8> },
    Directory { children: BTreeMap<String, Arc<RamNode>> },
}

impl RamNode {
    fn new_file() -> Self {
        RamNode {
            inner: RwLock::new(RamNodeInner::File {
                content: Vec::new(),
            }),
        }
    }

    fn new_dir() -> Self {
        RamNode {
            inner: RwLock::new(RamNodeInner::Directory {
                children: BTreeMap::new(),
            }),
        }
    }
}

impl Inode for RamNode {
    fn metadata(&self) -> Result<Metadata> {
        let inner = self.inner.read();
        match &*inner {
            RamNodeInner::File { content } => Ok(Metadata {
                file_type: FileType::File,
                size: content.len() as u64,
            }),
            RamNodeInner::Directory { .. } => Ok(Metadata {
                file_type: FileType::Directory,
                size: 0,
            }),
        }
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let inner = self.inner.read();
        match &*inner {
            RamNodeInner::File { content } => {
                let offset = offset as usize;
                if offset >= content.len() {
                    return Ok(0);
                }
                let len = core::cmp::min(buf.len(), content.len() - offset);
                buf[..len].copy_from_slice(&content[offset..offset + len]);
                Ok(len)
            }
            RamNodeInner::Directory { .. } => Err(VfsError::IsADirectory),
        }
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize> {
        let mut inner = self.inner.write();
        match &mut *inner {
            RamNodeInner::File { content } => {
                let offset = offset as usize;
                let end = offset + buf.len();
                if end > content.len() {
                    content.resize(end, 0);
                }
                content[offset..end].copy_from_slice(buf);
                Ok(buf.len())
            }
            RamNodeInner::Directory { .. } => Err(VfsError::IsADirectory),
        }
    }

    fn lookup(&self, name: &str) -> Result<Arc<dyn Inode>> {
        let inner = self.inner.read();
        match &*inner {
            RamNodeInner::Directory { children } => children
                .get(name)
                .map(|node| node.clone() as Arc<dyn Inode>)
                .ok_or(VfsError::NotFound),
            RamNodeInner::File { .. } => Err(VfsError::NotADirectory),
        }
    }

    fn create(&self, name: &str) -> Result<Arc<dyn Inode>> {
        let mut inner = self.inner.write();
        match &mut *inner {
            RamNodeInner::Directory { children } => {
                if children.contains_key(name) {
                    return Err(VfsError::AlreadyExists);
                }
                let node = Arc::new(RamNode::new_file());
                children.insert(name.to_string(), node.clone());
                Ok(node)
            }
            RamNodeInner::File { .. } => Err(VfsError::NotADirectory),
        }
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn Inode>> {
        let mut inner = self.inner.write();
        match &mut *inner {
            RamNodeInner::Directory { children } => {
                if children.contains_key(name) {
                    return Err(VfsError::AlreadyExists);
                }
                let node = Arc::new(RamNode::new_dir());
                children.insert(name.to_string(), node.clone());
                Ok(node)
            }
            RamNodeInner::File { .. } => Err(VfsError::NotADirectory),
        }
    }

    fn readdir(&self) -> Result<Vec<String>> {
        let inner = self.inner.read();
        match &*inner {
            RamNodeInner::Directory { children } => {
                Ok(children.keys().cloned().collect())
            }
            RamNodeInner::File { .. } => Err(VfsError::NotADirectory),
        }
    }
}
