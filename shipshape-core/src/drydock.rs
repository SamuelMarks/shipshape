//! CI configuration generation for ShipShape.
//!
//! Generates `Dockerfile` and `.gitlab-ci.yml` configurations tailored to the
//! build markers detected in a repository via [`FileSystem`].

use std::path::Path;

use crate::fs::{FileSystem, StdFileSystem, has_python_packaging, is_notebook_path};

/// Generate a `Dockerfile` and `.gitlab-ci.yml` configuration for a repository at `path`
/// using [`StdFileSystem`].
///
/// # Arguments
/// * `path` - Repository root directory.
#[must_use]
pub fn generate_ci_config(path: &Path) -> (String, String) {
    let fs = StdFileSystem::new();
    generate_ci_config_with_fs(&fs, path)
}

/// Generate a `Dockerfile` and `.gitlab-ci.yml` configuration for a repository at `path`
/// using the provided [`FileSystem`] adapter.
///
/// # Arguments
/// * `fs` - Filesystem implementation used to inspect repository markers and files.
/// * `path` - Repository root directory.
pub fn generate_ci_config_with_fs<F: FileSystem + ?Sized>(fs: &F, path: &Path) -> (String, String) {
    if detect_notebook_only(fs, path) {
        return (notebook_only_dockerfile(), notebook_only_ci());
    }
    match detect_language(fs, path) {
        Some(Language::Python) => (python_dockerfile(), python_ci()),
        Some(Language::Node) => (node_dockerfile(), node_ci()),
        Some(Language::Rust) => (rust_dockerfile(), rust_ci()),
        Some(Language::Go) => (go_dockerfile(), go_ci()),
        Some(Language::CMake) => (cmake_dockerfile(), cmake_ci()),
        None => (generic_dockerfile(), generic_ci()),
    }
}

/// Primary build stack detected from repository root marker files.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Language {
    /// Python project (`pyproject.toml`, `setup.py`, or `setup.cfg`).
    Python,
    /// Node.js project (`package.json`).
    Node,
    /// Rust project (`Cargo.toml`).
    Rust,
    /// Go project (`go.mod`).
    Go,
    /// C/C++ CMake project (`CMakeLists.txt`).
    CMake,
}

/// Detect the primary language stack from build marker files at `path`.
///
/// # Arguments
/// * `fs` - Filesystem implementation.
/// * `path` - Repository root path.
fn detect_language<F: FileSystem + ?Sized>(fs: &F, path: &Path) -> Option<Language> {
    if has_python_packaging(fs, path) {
        return Some(Language::Python);
    }
    if has_file(fs, path, "package.json") {
        return Some(Language::Node);
    }
    if has_file(fs, path, "Cargo.toml") {
        return Some(Language::Rust);
    }
    if has_file(fs, path, "go.mod") {
        return Some(Language::Go);
    }
    if has_file(fs, path, "CMakeLists.txt") {
        return Some(Language::CMake);
    }
    None
}

/// Check whether `root` contains a regular file named `name`.
///
/// # Arguments
/// * `fs` - Filesystem implementation.
/// * `root` - Directory path.
/// * `name` - File name relative to `root`.
fn has_file<F: FileSystem + ?Sized>(fs: &F, root: &Path, name: &str) -> bool {
    fs.is_file(&root.join(name))
}

/// Determine whether `root` contains Jupyter notebooks without Python packaging metadata.
///
/// # Arguments
/// * `fs` - Filesystem implementation.
/// * `root` - Repository root path.
fn detect_notebook_only<F: FileSystem + ?Sized>(fs: &F, root: &Path) -> bool {
    if has_python_packaging(fs, root) {
        return false;
    }
    contains_notebook(fs, root)
}

/// Return `true` if any non-hidden file under `root` is a `.ipynb` notebook.
///
/// # Arguments
/// * `fs` - Filesystem implementation.
/// * `root` - Repository root path.
fn contains_notebook<F: FileSystem + ?Sized>(fs: &F, root: &Path) -> bool {
    fs.list_files(root)
        .map(|files| files.iter().any(|path| is_notebook_path(path)))
        .unwrap_or(false)
}

/// Build the Python `Dockerfile` template.
fn python_dockerfile() -> String {
    [
        "FROM python:3.11-slim",
        "WORKDIR /app",
        "COPY . .",
        "RUN python -m pip install --upgrade pip",
        "RUN python -m pip install .[test]",
        r#"CMD ["pytest", "-q"]"#,
        "",
    ]
    .join("\n")
}

/// Build the Python `.gitlab-ci.yml` template.
fn python_ci() -> String {
    [
        "stages:",
        "  - test",
        "",
        "test:",
        "  image: python:3.11-slim",
        "  script:",
        "    - python -m pip install --upgrade pip",
        "    - python -m pip install .[test]",
        "    - pytest -q",
        "",
    ]
    .join("\n")
}

/// Build the Node.js `Dockerfile` template.
fn node_dockerfile() -> String {
    [
        "FROM node:20-alpine",
        "WORKDIR /app",
        "COPY . .",
        "RUN npm ci",
        r#"CMD ["npm", "test"]"#,
        "",
    ]
    .join("\n")
}

/// Build the Node.js `.gitlab-ci.yml` template.
fn node_ci() -> String {
    [
        "stages:",
        "  - test",
        "",
        "test:",
        "  image: node:20-alpine",
        "  script:",
        "    - npm ci",
        "    - npm test",
        "",
    ]
    .join("\n")
}

/// Build the Rust `Dockerfile` template.
fn rust_dockerfile() -> String {
    [
        "FROM rust:1.76",
        "WORKDIR /app",
        "COPY . .",
        "RUN cargo test --all",
        r#"CMD ["cargo", "test", "--all"]"#,
        "",
    ]
    .join("\n")
}

/// Build the Rust `.gitlab-ci.yml` template.
fn rust_ci() -> String {
    [
        "stages:",
        "  - test",
        "",
        "test:",
        "  image: rust:1.76",
        "  script:",
        "    - cargo test --all",
        "",
    ]
    .join("\n")
}

/// Build the Go `Dockerfile` template.
fn go_dockerfile() -> String {
    [
        "FROM golang:1.21",
        "WORKDIR /app",
        "COPY . .",
        "RUN go test ./...",
        r#"CMD ["go", "test", "./..."]"#,
        "",
    ]
    .join("\n")
}

/// Build the Go `.gitlab-ci.yml` template.
fn go_ci() -> String {
    [
        "stages:",
        "  - test",
        "",
        "test:",
        "  image: golang:1.21",
        "  script:",
        "    - go test ./...",
        "",
    ]
    .join("\n")
}

/// Build the CMake `Dockerfile` template.
fn cmake_dockerfile() -> String {
    [
        "FROM ubuntu:22.04",
        "WORKDIR /app",
        "COPY . .",
        "RUN apt-get update && apt-get install -y cmake build-essential",
        "RUN cmake -S . -B build",
        "RUN cmake --build build",
        "",
    ]
    .join("\n")
}

/// Build the CMake `.gitlab-ci.yml` template.
fn cmake_ci() -> String {
    [
        "stages:",
        "  - build",
        "",
        "build:",
        "  image: ubuntu:22.04",
        "  script:",
        "    - apt-get update",
        "    - apt-get install -y cmake build-essential",
        "    - cmake -S . -B build",
        "    - cmake --build build",
        "",
    ]
    .join("\n")
}

/// Build the fail-fast `Dockerfile` template for notebook-only repositories.
fn notebook_only_dockerfile() -> String {
    [
        "FROM ubuntu:22.04",
        "WORKDIR /app",
        "COPY . .",
        r#"RUN /bin/sh -c "echo 'Notebook-only repository detected; add packaging metadata.' && exit 1""#,
        "",
    ]
    .join("\n")
}

/// Build the fail-fast `.gitlab-ci.yml` template for notebook-only repositories.
fn notebook_only_ci() -> String {
    [
        "stages:",
        "  - validate",
        "",
        "validate:",
        "  image: ubuntu:22.04",
        "  script:",
        r#"    - echo "Notebook-only repository detected; add packaging metadata.""#,
        "    - exit 1",
        "",
    ]
    .join("\n")
}

/// Build the generic fallback `Dockerfile` template.
fn generic_dockerfile() -> String {
    [
        "FROM ubuntu:22.04",
        "WORKDIR /app",
        "COPY . .",
        r#"CMD ["/bin/sh", "-c", "echo 'No build markers detected.'"]"#,
        "",
    ]
    .join("\n")
}

/// Build the generic fallback `.gitlab-ci.yml` template.
fn generic_ci() -> String {
    [
        "stages:",
        "  - test",
        "",
        "test:",
        "  image: ubuntu:22.04",
        "  script:",
        r#"    - echo "No build markers detected.""#,
        "",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{generate_ci_config, generate_ci_config_with_fs};
    use crate::error::{Result, ShipShapeError};
    use crate::fs::MockFileSystem;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counter for unique test directories.
    static UNIQUE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn detects_python_projects() -> Result<()> {
        let root = temp_dir_with_marker("pyproject.toml")?;
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("python:3.11-slim"));
        assert!(ci.contains("pytest -q"));

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn detects_node_projects() -> Result<()> {
        let root = temp_dir_with_marker("package.json")?;
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("node:20-alpine"));
        assert!(ci.contains("npm test"));

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn detects_rust_projects() -> Result<()> {
        let root = temp_dir_with_marker("Cargo.toml")?;
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("rust:1.76"));
        assert!(ci.contains("cargo test --all"));

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn detects_go_projects() -> Result<()> {
        let root = temp_dir_with_marker("go.mod")?;
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("golang:1.21"));
        assert!(ci.contains("go test ./..."));

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn detects_cmake_projects() -> Result<()> {
        let root = temp_dir_with_marker("CMakeLists.txt")?;
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("cmake -S . -B build"));
        assert!(ci.contains("cmake --build build"));

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn notebook_only_projects_fail_fast() -> Result<()> {
        let root = temp_dir_with_marker("analysis.ipynb")?;
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("Notebook-only repository detected"));
        assert!(ci.contains("exit 1"));

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn notebooks_with_setup_cfg_use_python_config() -> Result<()> {
        let root = temp_dir_with_marker("analysis.ipynb")?;
        let setup_cfg = root.join("setup.cfg");
        std::fs::write(&setup_cfg, "[metadata]\nname = demo\n")?;
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("python:3.11-slim"));
        assert!(ci.contains("pytest -q"));

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn falls_back_when_no_markers_found_or_list_files_errors() -> Result<()> {
        let root = temp_dir_with_marker("README.md")?;
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("ubuntu:22.04"));
        assert!(ci.contains("No build markers detected"));
        std::fs::remove_dir_all(&root)?;

        let mut mock_fs = MockFileSystem::new();
        mock_fs.expect_is_file().returning(|_| false);
        mock_fs
            .expect_list_files()
            .returning(|_| Err(ShipShapeError::Other("io failure".to_string())));

        let (dockerfile, ci) = generate_ci_config_with_fs(&mock_fs, Path::new("/missing"));
        assert!(dockerfile.contains("No build markers detected"));
        assert!(ci.contains("No build markers detected"));
        Ok(())
    }

    /// Create a temporary directory containing `marker`.
    fn temp_dir_with_marker(marker: &str) -> Result<PathBuf> {
        let pid = std::process::id();
        let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("shipshape_drydock_test_{pid}_{counter}"));
        std::fs::create_dir_all(&root)?;
        let path = root.join(marker);
        std::fs::write(&path, "placeholder")?;
        Ok(root)
    }
}
