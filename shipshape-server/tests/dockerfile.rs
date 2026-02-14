//! Verifies the server Dockerfile includes required toolchains and mechanics.

use std::fs;
use std::path::PathBuf;

fn read_dockerfile() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Dockerfile");
    fs::read_to_string(&path).expect("read shipshape-server Dockerfile")
}

#[test]
fn dockerfile_installs_toolchains() {
    let dockerfile = read_dockerfile();
    let required = ["python3", "python3-pip", "clang", "golang-go"];
    for token in required {
        assert!(
            dockerfile.contains(token),
            "Dockerfile missing toolchain dependency: {token}"
        );
    }
}

#[test]
fn dockerfile_installs_mechanics() {
    let dockerfile = read_dockerfile();
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
}
