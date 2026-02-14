//! Filesystem abstractions used for inspection.

use std::path::{Path, PathBuf};

use crate::error::Result;

/// Abstraction over filesystem access for testability.
#[cfg_attr(test, mockall::automock)]
pub trait FileSystem {
    /// List all files reachable from the root path.
    fn list_files(&self, root: &Path) -> Result<Vec<PathBuf>>;
    /// Read a file into a string.
    fn read_to_string(&self, path: &Path) -> Result<String>;
}

/// Default filesystem implementation backed by `std::fs`.
#[derive(Debug, Default, Clone)]
pub struct StdFileSystem;

impl StdFileSystem {
    /// Create a new standard filesystem adapter.
    pub fn new() -> Self {
        Self
    }
}

impl FileSystem for StdFileSystem {
    fn list_files(&self, root: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];

        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if is_hidden(&path) {
                    continue;
                }
                let file_type = entry.file_type()?;
                if file_type.is_dir() {
                    pending.push(path);
                } else if file_type.is_file() {
                    files.push(path);
                }
            }
        }

        Ok(files)
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::StdFileSystem;
    use crate::fs::FileSystem;
    use std::path::PathBuf;

    #[test]
    fn std_filesystem_lists_and_reads_files() {
        let root = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&root).expect("create temp dir");
        let file_path = root.join("hello.txt");
        std::fs::write(&file_path, "hello shipshape").expect("write test file");

        let fs = StdFileSystem::new();
        let files = fs.list_files(&root).expect("list files");
        assert_eq!(files, vec![file_path.clone()]);

        let contents = fs.read_to_string(&file_path).expect("read file");
        assert_eq!(contents, "hello shipshape");

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    fn unique_dir_name() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        PathBuf::from(format!("shipshape_core_test_{nanos}"))
    }
}
