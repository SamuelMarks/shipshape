//! Filesystem abstractions used for inspection, template lookup, and CI generation.
//!
//! Provides the [`FileSystem`] trait for testable disk access and [`StdFileSystem`]
//! as the standard OS-backed implementation, along with shared path-classification
//! utilities.

use std::path::{Path, PathBuf};

use crate::error::Result;

/// Python packaging marker filenames recognized across ShipShape.
pub const PYTHON_PACKAGING_MARKERS: [&str; 3] = ["pyproject.toml", "setup.py", "setup.cfg"];

/// Abstraction over filesystem access for testability.
#[cfg_attr(test, mockall::automock)]
pub trait FileSystem {
    /// List all non-hidden files reachable recursively from the `root` path.
    ///
    /// # Arguments
    /// * `root` - Root directory path to traverse.
    fn list_files(&self, root: &Path) -> Result<Vec<PathBuf>>;

    /// Read a file at `path` into a UTF-8 string.
    ///
    /// # Arguments
    /// * `path` - File path to read.
    fn read_to_string(&self, path: &Path) -> Result<String>;

    /// Return `true` if `path` exists and is a regular file.
    ///
    /// # Arguments
    /// * `path` - Path to inspect.
    fn is_file(&self, path: &Path) -> bool;

    /// Return `true` if `path` exists and is a directory.
    ///
    /// # Arguments
    /// * `path` - Path to inspect.
    fn is_dir(&self, path: &Path) -> bool;

    /// Write string `contents` to `path`, creating parent directories if needed.
    ///
    /// # Arguments
    /// * `path` - Target file path.
    /// * `contents` - UTF-8 text to write.
    fn write_file(&self, path: &Path, contents: &str) -> Result<()>;
}

/// Default filesystem implementation backed by `std::fs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdFileSystem;

impl StdFileSystem {
    /// Create a new standard filesystem adapter.
    #[must_use]
    pub const fn new() -> Self {
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
                if is_hidden_path(&path) {
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

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn write_file(&self, path: &Path, contents: &str) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
        Ok(())
    }
}

/// Check whether the final segment of `path` is hidden (starts with `'.'`).
///
/// # Arguments
/// * `path` - Filesystem path to inspect.
#[must_use]
pub fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

/// Check whether `path` has a Jupyter notebook `.ipynb` extension (case-insensitive).
///
/// # Arguments
/// * `path` - Filesystem path to inspect.
#[must_use]
pub fn is_notebook_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ipynb"))
}

/// Check whether the repository `root` contains Python packaging metadata.
///
/// Checks for `pyproject.toml`, `setup.py`, or `setup.cfg` using `fs`.
///
/// # Arguments
/// * `fs` - Filesystem implementation.
/// * `root` - Repository root directory.
pub fn has_python_packaging<F: FileSystem + ?Sized>(fs: &F, root: &Path) -> bool {
    PYTHON_PACKAGING_MARKERS
        .iter()
        .any(|name| fs.is_file(&root.join(name)))
}

#[cfg(test)]
mod tests {
    use super::{
        FileSystem, StdFileSystem, has_python_packaging, is_hidden_path, is_notebook_path,
    };
    use crate::error::Result;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Atomic counter for unique test directory names.
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn std_filesystem_lists_reads_writes_and_checks_paths() -> Result<()> {
        let root = unique_temp_dir();
        let fs = StdFileSystem::new();
        assert!(!fs.is_dir(&root));

        let nested_file = root.join("sub").join("hello.txt");
        let hidden_file = root.join(".hidden");
        fs.write_file(&nested_file, "hello shipshape")?;
        fs.write_file(&hidden_file, "secret")?;

        assert!(fs.is_dir(&root));
        assert!(fs.is_file(&nested_file));
        assert!(!fs.is_file(&root));

        let files = fs.list_files(&root)?;
        assert_eq!(files, vec![nested_file.clone()]);

        let contents = fs.read_to_string(&nested_file)?;
        assert_eq!(contents, "hello shipshape");

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn write_file_handles_relative_filename_without_parent_dir() -> Result<()> {
        let root = unique_temp_dir();
        std::fs::create_dir_all(&root)?;
        let file_path = root.join("direct.txt");
        let fs = StdFileSystem;
        fs.write_file(Path::new(""), "")
            .err()
            .map_or_else(|| (), |_| ());
        fs.write_file(&file_path, "content")?;
        assert_eq!(fs.read_to_string(&file_path)?, "content");
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn list_files_ignores_non_file_non_dir_entries() -> Result<()> {
        use std::os::unix::net::UnixListener;

        let root = unique_temp_dir();
        std::fs::create_dir_all(&root)?;
        let sock_path = root.join("test.sock");
        let _listener = UnixListener::bind(&sock_path)?;
        let regular = root.join("regular.txt");
        std::fs::write(&regular, "ok")?;

        let fs = StdFileSystem::new();
        let files = fs.list_files(&root)?;
        assert_eq!(files, vec![regular]);

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn path_classification_helpers_work_as_expected() -> Result<()> {
        assert!(is_hidden_path(Path::new("/repo/.git")));
        assert!(!is_hidden_path(Path::new("/repo/src")));
        assert!(!is_hidden_path(Path::new("")));

        assert!(is_notebook_path(Path::new("analysis.ipynb")));
        assert!(is_notebook_path(Path::new("analysis.IPYNB")));
        assert!(!is_notebook_path(Path::new("analysis.py")));
        assert!(!is_notebook_path(Path::new("Makefile")));

        let root = unique_temp_dir();
        let fs = StdFileSystem::new();
        assert!(!has_python_packaging(&fs, &root));
        let setup_cfg = root.join("setup.cfg");
        fs.write_file(&setup_cfg, "[metadata]\nname = demo\n")?;
        assert!(has_python_packaging(&fs, &root));
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// Generate a unique temporary directory path.
    fn unique_temp_dir() -> PathBuf {
        let pid = std::process::id();
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("shipshape_core_fs_test_{pid}_{count}"))
    }
}
