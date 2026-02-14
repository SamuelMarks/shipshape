//! CI configuration generation for ShipShape.

use std::path::Path;

/// Generate a Dockerfile and `.gitlab-ci.yml` configuration for a repository.
///
/// The generated output is based on build marker files found at the repository root.
pub fn generate_ci_config(path: &Path) -> (String, String) {
    if detect_notebook_only(path) {
        return (notebook_only_dockerfile(), notebook_only_ci());
    }
    match detect_language(path) {
        Some(Language::Python) => (python_dockerfile(), python_ci()),
        Some(Language::Node) => (node_dockerfile(), node_ci()),
        Some(Language::Rust) => (rust_dockerfile(), rust_ci()),
        Some(Language::Go) => (go_dockerfile(), go_ci()),
        Some(Language::CMake) => (cmake_dockerfile(), cmake_ci()),
        None => (generic_dockerfile(), generic_ci()),
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Language {
    Python,
    Node,
    Rust,
    Go,
    CMake,
}

fn detect_language(path: &Path) -> Option<Language> {
    if has_file(path, "pyproject.toml") || has_file(path, "setup.py") {
        return Some(Language::Python);
    }
    if has_file(path, "package.json") {
        return Some(Language::Node);
    }
    if has_file(path, "Cargo.toml") {
        return Some(Language::Rust);
    }
    if has_file(path, "go.mod") {
        return Some(Language::Go);
    }
    if has_file(path, "CMakeLists.txt") {
        return Some(Language::CMake);
    }
    None
}

fn has_file(root: &Path, name: &str) -> bool {
    root.join(name).is_file()
}

fn detect_notebook_only(root: &Path) -> bool {
    if has_python_packaging(root) {
        return false;
    }
    contains_notebook(root)
}

fn contains_notebook(root: &Path) -> bool {
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if is_hidden(&path) {
                continue;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && is_notebook(&path) {
                return true;
            }
        }
    }
    false
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

fn is_notebook(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("ipynb"))
        .unwrap_or(false)
}

fn has_python_packaging(root: &Path) -> bool {
    ["pyproject.toml", "setup.py", "setup.cfg"]
        .iter()
        .any(|name| root.join(name).is_file())
}

fn python_dockerfile() -> String {
    [
        "FROM python:3.11-slim",
        "WORKDIR /app",
        "COPY . .",
        "RUN python -m pip install --upgrade pip",
        "RUN python -m pip install .[test]",
        "CMD [\"pytest\", \"-q\"]",
        "",
    ]
    .join("\n")
}

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

fn node_dockerfile() -> String {
    [
        "FROM node:20-alpine",
        "WORKDIR /app",
        "COPY . .",
        "RUN npm ci",
        "CMD [\"npm\", \"test\"]",
        "",
    ]
    .join("\n")
}

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

fn rust_dockerfile() -> String {
    [
        "FROM rust:1.76",
        "WORKDIR /app",
        "COPY . .",
        "RUN cargo test --all",
        "CMD [\"cargo\", \"test\", \"--all\"]",
        "",
    ]
    .join("\n")
}

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

fn go_dockerfile() -> String {
    [
        "FROM golang:1.21",
        "WORKDIR /app",
        "COPY . .",
        "RUN go test ./...",
        "CMD [\"go\", \"test\", \"./...\"]",
        "",
    ]
    .join("\n")
}

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

fn notebook_only_dockerfile() -> String {
    [
        "FROM ubuntu:22.04",
        "WORKDIR /app",
        "COPY . .",
        "RUN /bin/sh -c \"echo 'Notebook-only repository detected; add packaging metadata.' && exit 1\"",
        "",
    ]
    .join("\n")
}

fn notebook_only_ci() -> String {
    [
        "stages:",
        "  - validate",
        "",
        "validate:",
        "  image: ubuntu:22.04",
        "  script:",
        "    - echo \"Notebook-only repository detected; add packaging metadata.\"",
        "    - exit 1",
        "",
    ]
    .join("\n")
}

fn generic_dockerfile() -> String {
    [
        "FROM ubuntu:22.04",
        "WORKDIR /app",
        "COPY . .",
        "CMD [\"/bin/sh\", \"-c\", \"echo 'No build markers detected.'\"]",
        "",
    ]
    .join("\n")
}

fn generic_ci() -> String {
    [
        "stages:",
        "  - test",
        "",
        "test:",
        "  image: ubuntu:22.04",
        "  script:",
        "    - echo \"No build markers detected.\"",
        "",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::generate_ci_config;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static UNIQUE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn detects_python_projects() {
        let root = temp_dir_with_marker("pyproject.toml");
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("python:3.11-slim"));
        assert!(ci.contains("pytest -q"));

        cleanup_dir(&root);
    }

    #[test]
    fn detects_node_projects() {
        let root = temp_dir_with_marker("package.json");
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("node:20-alpine"));
        assert!(ci.contains("npm test"));

        cleanup_dir(&root);
    }

    #[test]
    fn detects_rust_projects() {
        let root = temp_dir_with_marker("Cargo.toml");
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("rust:1.76"));
        assert!(ci.contains("cargo test --all"));

        cleanup_dir(&root);
    }

    #[test]
    fn detects_go_projects() {
        let root = temp_dir_with_marker("go.mod");
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("golang:1.21"));
        assert!(ci.contains("go test ./..."));

        cleanup_dir(&root);
    }

    #[test]
    fn detects_cmake_projects() {
        let root = temp_dir_with_marker("CMakeLists.txt");
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("cmake -S . -B build"));
        assert!(ci.contains("cmake --build build"));

        cleanup_dir(&root);
    }

    #[test]
    fn notebook_only_projects_fail_fast() {
        let root = temp_dir_with_marker("analysis.ipynb");
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("Notebook-only repository detected"));
        assert!(ci.contains("exit 1"));

        cleanup_dir(&root);
    }

    #[test]
    fn notebooks_with_packaging_use_python_config() {
        let root = temp_dir_with_marker("analysis.ipynb");
        std::fs::write(root.join("pyproject.toml"), "name = \"demo\"").expect("write pyproject");
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("python:3.11-slim"));
        assert!(ci.contains("pytest -q"));

        cleanup_dir(&root);
    }

    #[test]
    fn falls_back_when_no_markers_found() {
        let root = temp_dir_with_marker("README.md");
        let (dockerfile, ci) = generate_ci_config(&root);

        assert!(dockerfile.contains("ubuntu:22.04"));
        assert!(ci.contains("No build markers detected"));

        cleanup_dir(&root);
    }

    fn temp_dir_with_marker(marker: &str) -> PathBuf {
        let root = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&root).expect("create temp dir");
        let path = root.join(marker);
        std::fs::write(&path, "placeholder").expect("write marker");
        root
    }

    fn unique_dir_name() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        PathBuf::from(format!("shipshape_drydock_test_{nanos}_{counter}"))
    }

    fn cleanup_dir(root: &Path) {
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
