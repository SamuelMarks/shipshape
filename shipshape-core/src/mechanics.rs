//! Mechanic registry and external tool wrappers.

use crate::{FileSystem, Mechanic, Result, ShipShapeError, StdFileSystem, Violation};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Build mechanic instances from a list of IDs.
pub fn build_mechanics(ids: &[String]) -> Result<Vec<Arc<dyn Mechanic + Send + Sync>>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut mechanics: Vec<Arc<dyn Mechanic + Send + Sync>> = Vec::new();
    for id in ids {
        let mechanic = match normalize_mechanic_id(id) {
            Some(MechanicKind::Noop) => Arc::new(NoopMechanic) as Arc<_>,
            Some(MechanicKind::Notebook) => Arc::new(NotebookMechanic::new()) as Arc<_>,
            Some(MechanicKind::TypeCorrect) => Arc::new(type_correct_mechanic()) as Arc<_>,
            Some(MechanicKind::CddC) => Arc::new(cdd_c_mechanic()) as Arc<_>,
            Some(MechanicKind::GoAutoErrHandling) => {
                Arc::new(go_auto_err_handling_mechanic()) as Arc<_>
            }
            None => return Err(ShipShapeError::Other(format!("unknown mechanic: {id}"))),
        };
        mechanics.push(mechanic);
    }

    Ok(mechanics)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum MechanicKind {
    Noop,
    Notebook,
    TypeCorrect,
    CddC,
    GoAutoErrHandling,
}

fn normalize_mechanic_id(id: &str) -> Option<MechanicKind> {
    match id.trim().to_lowercase().as_str() {
        "noop" => Some(MechanicKind::Noop),
        "lib2nb2lib" | "lib2notebook2lib" | "notebook-cleaner" => Some(MechanicKind::Notebook),
        "type-correct" | "cpp-types" => Some(MechanicKind::TypeCorrect),
        "cdd-c" | "c-error-handling" => Some(MechanicKind::CddC),
        "go-auto-err-handling" | "go-error-handling" => Some(MechanicKind::GoAutoErrHandling),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct CommandSpec {
    program: &'static str,
    args: &'static [&'static str],
}

impl CommandSpec {
    fn run(&self, path: &Path) -> Result<CommandOutput> {
        let output = Command::new(self.program)
            .args(self.args)
            .arg(path)
            .output()
            .map_err(ShipShapeError::from)?;

        Ok(CommandOutput::from(output))
    }
}

#[derive(Debug, Clone)]
struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

impl CommandOutput {
    fn from(output: std::process::Output) -> Self {
        Self {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }
    }

    fn merged_output(&self) -> String {
        let mut merged = String::new();
        if !self.stdout.trim().is_empty() {
            merged.push_str(self.stdout.trim());
        }
        if !self.stderr.trim().is_empty() {
            if !merged.is_empty() {
                merged.push('\n');
            }
            merged.push_str(self.stderr.trim());
        }
        merged
    }
}

#[derive(Debug, Clone)]
struct ExternalMechanic {
    id: &'static str,
    audit: Option<CommandSpec>,
    dry_run: Option<CommandSpec>,
    apply: Option<CommandSpec>,
}

impl ExternalMechanic {
    fn audit_with(&self, path: &Path) -> Result<Vec<Violation>> {
        let Some(spec) = &self.audit else {
            return Err(ShipShapeError::Other(format!(
                "audit not configured for {}",
                self.id
            )));
        };
        let output = spec.run(path)?;
        let merged = output.merged_output();
        if !output.status.success() && merged.is_empty() {
            return Err(ShipShapeError::Other(format!(
                "{} failed with status {}",
                self.id, output.status
            )));
        }

        Ok(output_to_violations(self.id, &merged))
    }

    fn dry_run_with(&self, path: &Path) -> Result<String> {
        let Some(spec) = &self.dry_run else {
            return Err(ShipShapeError::Other(format!(
                "dry-run not configured for {}",
                self.id
            )));
        };
        let output = spec.run(path)?;
        let merged = output.merged_output();
        if !output.status.success() && merged.is_empty() {
            return Err(ShipShapeError::Other(format!(
                "{} failed with status {}",
                self.id, output.status
            )));
        }
        Ok(merged)
    }

    fn apply_with(&self, path: &Path) -> Result<bool> {
        let Some(spec) = &self.apply else {
            return Err(ShipShapeError::Other(format!(
                "apply not configured for {}",
                self.id
            )));
        };
        let output = spec.run(path)?;
        if !output.status.success() {
            let merged = output.merged_output();
            let detail = if merged.is_empty() {
                format!("{}", output.status)
            } else {
                merged
            };
            return Err(ShipShapeError::Other(format!(
                "{} failed: {detail}",
                self.id
            )));
        }
        Ok(output.status.success())
    }
}

fn output_to_violations(id: &str, output: &str) -> Vec<Violation> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| Violation {
            id: id.to_string(),
            message: line.to_string(),
        })
        .collect()
}

#[derive(Debug, Clone)]
struct NotebookMechanic {
    converter: ExternalMechanic,
}

impl NotebookMechanic {
    fn new() -> Self {
        Self {
            converter: ExternalMechanic {
                id: "lib2nb2lib",
                audit: None,
                dry_run: None,
                apply: Some(CommandSpec {
                    program: "lib2notebook2lib",
                    args: &["--convert"],
                }),
            },
        }
    }

    fn detect_notebook_only_repo(&self, path: &Path) -> Result<bool> {
        let fs = StdFileSystem::new();
        let files = fs.list_files(path)?;
        let mut has_notebooks = false;
        for file in files {
            if is_notebook(&file) {
                has_notebooks = true;
                break;
            }
        }
        if !has_notebooks {
            return Ok(false);
        }

        if has_python_packaging(path) {
            return Ok(false);
        }

        Ok(true)
    }
}

impl Mechanic for NotebookMechanic {
    fn id(&self) -> &str {
        self.converter.id
    }

    fn audit(&self, path: &Path) -> Result<Vec<Violation>> {
        if self.detect_notebook_only_repo(path)? {
            return Ok(vec![Violation {
                id: self.id().to_string(),
                message: "Jupyter notebooks detected without packaging metadata (pyproject.toml, setup.py, setup.cfg).".to_string(),
            }]);
        }
        Ok(Vec::new())
    }

    fn dry_run(&self, _path: &Path) -> Result<String> {
        Err(ShipShapeError::Other(
            "lib2notebook2lib dry-run not configured".to_string(),
        ))
    }

    fn apply(&self, path: &Path) -> Result<bool> {
        self.converter.apply_with(path)
    }
}

#[derive(Debug, Clone)]
struct TypeCorrectMechanic {
    inner: ExternalMechanic,
}

impl Mechanic for TypeCorrectMechanic {
    fn id(&self) -> &str {
        self.inner.id
    }

    fn audit(&self, path: &Path) -> Result<Vec<Violation>> {
        self.inner.audit_with(path)
    }

    fn dry_run(&self, path: &Path) -> Result<String> {
        self.inner.dry_run_with(path)
    }

    fn apply(&self, path: &Path) -> Result<bool> {
        self.inner.apply_with(path)
    }
}

#[derive(Debug, Clone)]
struct CddCMechanic {
    inner: ExternalMechanic,
}

impl Mechanic for CddCMechanic {
    fn id(&self) -> &str {
        self.inner.id
    }

    fn audit(&self, path: &Path) -> Result<Vec<Violation>> {
        self.inner.audit_with(path)
    }

    fn dry_run(&self, path: &Path) -> Result<String> {
        self.inner.dry_run_with(path)
    }

    fn apply(&self, path: &Path) -> Result<bool> {
        self.inner.apply_with(path)
    }
}

#[derive(Debug, Clone)]
struct GoAutoErrHandlingMechanic {
    inner: ExternalMechanic,
}

impl Mechanic for GoAutoErrHandlingMechanic {
    fn id(&self) -> &str {
        self.inner.id
    }

    fn audit(&self, path: &Path) -> Result<Vec<Violation>> {
        self.inner.audit_with(path)
    }

    fn dry_run(&self, path: &Path) -> Result<String> {
        self.inner.dry_run_with(path)
    }

    fn apply(&self, path: &Path) -> Result<bool> {
        self.inner.apply_with(path)
    }
}

fn type_correct_mechanic() -> TypeCorrectMechanic {
    TypeCorrectMechanic {
        inner: ExternalMechanic {
            id: "type-correct",
            audit: Some(CommandSpec {
                program: "type-correct",
                args: &["--dry-run"],
            }),
            dry_run: Some(CommandSpec {
                program: "type-correct",
                args: &["--dry-run"],
            }),
            apply: Some(CommandSpec {
                program: "type-correct",
                args: &[],
            }),
        },
    }
}

fn cdd_c_mechanic() -> CddCMechanic {
    CddCMechanic {
        inner: ExternalMechanic {
            id: "cdd-c",
            audit: Some(CommandSpec {
                program: "cdd-c",
                args: &["--audit"],
            }),
            dry_run: Some(CommandSpec {
                program: "cdd-c",
                args: &["--audit"],
            }),
            apply: Some(CommandSpec {
                program: "cdd-c",
                args: &[],
            }),
        },
    }
}

fn go_auto_err_handling_mechanic() -> GoAutoErrHandlingMechanic {
    GoAutoErrHandlingMechanic {
        inner: ExternalMechanic {
            id: "go-auto-err-handling",
            audit: Some(CommandSpec {
                program: "go-auto-err-handling",
                args: &["--audit"],
            }),
            dry_run: Some(CommandSpec {
                program: "go-auto-err-handling",
                args: &["--audit"],
            }),
            apply: Some(CommandSpec {
                program: "go-auto-err-handling",
                args: &[],
            }),
        },
    }
}

struct NoopMechanic;

impl Mechanic for NoopMechanic {
    fn id(&self) -> &str {
        "noop"
    }

    fn audit(&self, _path: &Path) -> Result<Vec<Violation>> {
        Ok(Vec::new())
    }

    fn dry_run(&self, _path: &Path) -> Result<String> {
        Ok(String::from("noop"))
    }

    fn apply(&self, _path: &Path) -> Result<bool> {
        Ok(false)
    }
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

#[cfg(test)]
mod tests {
    use super::{NotebookMechanic, build_mechanics};
    use crate::Mechanic;
    use std::path::PathBuf;

    #[test]
    fn notebook_audit_reports_violation_when_no_packaging() {
        let root = temp_dir_with_file("analysis.ipynb");
        let mechanic = NotebookMechanic::new();
        let violations = mechanic.audit(&root).expect("audit notebook");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].id, "lib2nb2lib");

        cleanup_dir(&root);
    }

    #[test]
    fn notebook_audit_skips_when_packaging_present() {
        let root = temp_dir_with_file("analysis.ipynb");
        std::fs::write(root.join("pyproject.toml"), "name = \"demo\"").expect("write pyproject");

        let mechanic = NotebookMechanic::new();
        let violations = mechanic.audit(&root).expect("audit notebook");

        assert!(violations.is_empty());

        cleanup_dir(&root);
    }

    #[test]
    fn build_mechanics_supports_aliases() {
        let mechanics = build_mechanics(&vec![
            "lib2notebook2lib".to_string(),
            "cpp-types".to_string(),
            "go-auto-err-handling".to_string(),
        ])
        .expect("build mechanics");

        let ids: Vec<&str> = mechanics.iter().map(|m| m.id()).collect();
        assert!(ids.contains(&"lib2nb2lib"));
        assert!(ids.contains(&"type-correct"));
        assert!(ids.contains(&"go-auto-err-handling"));
    }

    fn temp_dir_with_file(filename: &str) -> PathBuf {
        let root = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&root).expect("create temp dir");
        let path = root.join(filename);
        std::fs::write(path, "placeholder").expect("write file");
        root
    }

    fn unique_dir_name() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        PathBuf::from(format!("shipshape_core_mechanic_test_{nanos}"))
    }

    fn cleanup_dir(root: &PathBuf) {
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
