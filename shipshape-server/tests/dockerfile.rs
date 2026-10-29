#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
//! Verifies the server `Dockerfile` includes required toolchains and mechanics.

use std::fs;
use std::io;
use std::path::PathBuf;

/// Read the `shipshape-server/Dockerfile` contents from disk.
fn read_dockerfile() -> io::Result<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Dockerfile");
    fs::read_to_string(&path)
}

#[test]
fn dockerfile_installs_toolchains() -> io::Result<()> {
    let dockerfile = read_dockerfile()?;
    let required = ["python3", "python3-pip", "clang", "golang-go"];
    for token in required {
        assert!(
            dockerfile.contains(token),
            "Dockerfile missing toolchain dependency: {token}"
        );
    }
    Ok(())
}

#[test]
fn dockerfile_installs_mechanics() -> io::Result<()> {
    let dockerfile = read_dockerfile()?;
    let mechanics = [
        "cdd-c",
        "type-correct",
        "lib2notebook2lib",
        "go-auto-err-handling",
    ];
    for mechanic in mechanics {
        assert!(
            dockerfile.contains(mechanic),
            "Dockerfile missing mechanic install: {mechanic}"
        );
    }
    Ok(())
}
