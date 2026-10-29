//! Mechanic registry and external tool wrappers.
//!
//! Provides [`build_mechanics`] to construct [`Mechanic`] instances by canonical ID
//! or alias, [`CommandRunner`] for testable subprocess execution, and implementations
//! for notebook conversion, C++ type correction, C error handling, Go error handling,
//! CI drydock generation, and no-op mechanics.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::drydock::generate_ci_config_with_fs;
use crate::fs::{FileSystem, StdFileSystem, has_python_packaging, is_notebook_path};
use crate::{Mechanic, Result, ShipShapeError, Violation};

/// Build mechanic instances from a list of mechanic identifiers or aliases.
///
/// # Arguments
/// * `ids` - Slice of mechanic identifiers or aliases (e.g., `"type-correct"`, `"cpp-types"`,
///   `"lib2nb2lib"`, `"notebook-lib"`, `"cdd-c"`, `"go-auto-err-handling"`, `"go-err"`,
///   `"ci-drydock"`, `"noop"`).
pub fn build_mechanics(ids: &[String]) -> Result<Vec<Arc<dyn Mechanic + Send + Sync>>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let runner: Arc<dyn CommandRunner> = Arc::new(StdCommandRunner);
    let fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(StdFileSystem::new());
    build_mechanics_with(ids, &runner, &fs)
}

/// Build mechanic instances with injected [`CommandRunner`] and [`FileSystem`] dependencies.
///
/// # Arguments
/// * `ids` - Slice of mechanic identifiers or aliases.
/// * `runner` - Subprocess runner implementation.
/// * `fs` - Filesystem implementation.
pub fn build_mechanics_with(
    ids: &[String],
    runner: &Arc<dyn CommandRunner>,
    fs: &Arc<dyn FileSystem + Send + Sync>,
) -> Result<Vec<Arc<dyn Mechanic + Send + Sync>>> {
    let mut mechanics: Vec<Arc<dyn Mechanic + Send + Sync>> = Vec::with_capacity(ids.len());
    for id in ids {
        let mechanic: Arc<dyn Mechanic + Send + Sync> = match normalize_mechanic_id(id) {
            Some(MechanicKind::Noop) => Arc::new(NoopMechanic),
            Some(MechanicKind::Notebook) => Arc::new(NotebookMechanic::with_deps(
                Arc::clone(runner),
                Arc::clone(fs),
            )),
            Some(MechanicKind::TypeCorrect) => Arc::new(type_correct_mechanic(Arc::clone(runner))),
            Some(MechanicKind::CddC) => Arc::new(cdd_c_mechanic(Arc::clone(runner))),
            Some(MechanicKind::GoAutoErrHandling) => {
                Arc::new(go_auto_err_handling_mechanic(Arc::clone(runner)))
            }
            Some(MechanicKind::CiDrydock) => Arc::new(CiDrydockMechanic::new(Arc::clone(fs))),
            None => {
                return Err(ShipShapeError::UnknownMechanic { id: id.clone() });
            }
        };
        mechanics.push(mechanic);
    }

    Ok(mechanics)
}

/// Canonical mechanic kinds supported by ShipShape.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum MechanicKind {
    /// No-operation mechanic for testing and dry pipelines.
    Noop,
    /// Jupyter notebook to Python library converter (`lib2nb2lib`).
    Notebook,
    /// C++ type-correctness analyzer (`type-correct`).
    TypeCorrect,
    /// C error-handling synthesizer (`cdd-c`).
    CddC,
    /// Go automatic error-handling injector (`go-auto-err-handling`).
    GoAutoErrHandling,
    /// Dockerfile and GitLab CI synthesizer (`ci-drydock`).
    CiDrydock,
}

/// Normalize a user-supplied mechanic identifier or alias to a [`MechanicKind`].
///
/// # Arguments
/// * `id` - Raw mechanic identifier string.
fn normalize_mechanic_id(id: &str) -> Option<MechanicKind> {
    match id.trim().to_lowercase().as_str() {
        "noop" => Some(MechanicKind::Noop),
        "lib2nb2lib" | "lib2notebook2lib" | "notebook-cleaner" | "notebook-lib" => {
            Some(MechanicKind::Notebook)
        }
        "type-correct" | "cpp-types" => Some(MechanicKind::TypeCorrect),
        "cdd-c" | "c-error-handling" => Some(MechanicKind::CddC),
        "go-auto-err-handling" | "go-error-handling" | "go-err" => {
            Some(MechanicKind::GoAutoErrHandling)
        }
        "ci-drydock" | "drydock" => Some(MechanicKind::CiDrydock),
        _ => None,
    }
}

/// Output captured from an external mechanic command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Whether the command exited with code 0.
    pub success: bool,
    /// Human-readable status description (e.g., `"exit status: 1"`).
    pub status_label: String,
    /// Captured standard output text.
    pub stdout: String,
    /// Captured standard error text.
    pub stderr: String,
}

impl CommandOutput {
    /// Convert a standard library [`std::process::Output`] into a [`CommandOutput`].
    ///
    /// # Arguments
    /// * `output` - Process output returned by `std::process::Command::output`.
    #[must_use]
    pub fn from_process_output(output: &std::process::Output) -> Self {
        Self {
            success: output.status.success(),
            status_label: output.status.to_string(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }
    }

    /// Combine trimmed `stdout` and `stderr` streams into a single diagnostic string.
    #[must_use]
    pub fn merged_output(&self) -> String {
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

/// Abstraction over external subprocess execution for mechanics.
pub trait CommandRunner: Send + Sync {
    /// Execute `program` with `args` followed by the repository `path`.
    ///
    /// # Arguments
    /// * `program` - Executable name or path.
    /// * `args` - Static CLI flags passed before `path`.
    /// * `path` - Target repository path argument.
    fn run(&self, program: &str, args: &[&str], path: &Path) -> Result<CommandOutput>;
}

/// Standard OS process runner backed by [`std::process::Command`].
#[derive(Debug, Default, Clone, Copy)]
pub struct StdCommandRunner;

impl CommandRunner for StdCommandRunner {
    fn run(&self, program: &str, args: &[&str], path: &Path) -> Result<CommandOutput> {
        let output = Command::new(program).args(args).arg(path).output()?;
        Ok(CommandOutput::from_process_output(&output))
    }
}

/// Specification for invoking an external CLI mechanic binary.
#[derive(Debug, Clone, Copy)]
struct CommandSpec {
    /// Executable name.
    program: &'static str,
    /// Command-line arguments preceding the repository path.
    args: &'static [&'static str],
}

/// Mechanic backed by external CLI commands for audit, dry-run, and apply stages.
#[derive(Clone)]
struct ExternalMechanic {
    /// Canonical mechanic identifier.
    id: &'static str,
    /// Optional command specification for `audit`.
    audit: Option<CommandSpec>,
    /// Optional command specification for `dry_run`.
    dry_run: Option<CommandSpec>,
    /// Optional command specification for `apply`.
    apply: Option<CommandSpec>,
    /// Subprocess runner.
    runner: Arc<dyn CommandRunner>,
}

impl ExternalMechanic {
    /// Run the configured `audit` command and parse output lines into [`Violation`]s.
    ///
    /// # Arguments
    /// * `path` - Repository root path.
    fn audit_with(&self, path: &Path) -> Result<Vec<Violation>> {
        let Some(spec) = &self.audit else {
            return Err(ShipShapeError::MechanicNotConfigured {
                id: self.id.to_string(),
                operation: "audit",
            });
        };
        let output = self.runner.run(spec.program, spec.args, path)?;
        let merged = output.merged_output();
        if !output.success && merged.is_empty() {
            return Err(ShipShapeError::MechanicExecutionFailed {
                id: self.id.to_string(),
                status: output.status_label,
                detail: "no output".to_string(),
            });
        }

        Ok(output_to_violations(self.id, &merged))
    }

    /// Run the configured `dry_run` command and return the merged diff output.
    ///
    /// # Arguments
    /// * `path` - Repository root path.
    fn dry_run_with(&self, path: &Path) -> Result<String> {
        let Some(spec) = &self.dry_run else {
            return Err(ShipShapeError::MechanicNotConfigured {
                id: self.id.to_string(),
                operation: "dry-run",
            });
        };
        let output = self.runner.run(spec.program, spec.args, path)?;
        let merged = output.merged_output();
        if !output.success && merged.is_empty() {
            return Err(ShipShapeError::MechanicExecutionFailed {
                id: self.id.to_string(),
                status: output.status_label,
                detail: "no output".to_string(),
            });
        }
        Ok(merged)
    }

    /// Run the configured `apply` command and return `true` on success.
    ///
    /// # Arguments
    /// * `path` - Repository root path.
    fn apply_with(&self, path: &Path) -> Result<bool> {
        let Some(spec) = &self.apply else {
            return Err(ShipShapeError::MechanicNotConfigured {
                id: self.id.to_string(),
                operation: "apply",
            });
        };
        let output = self.runner.run(spec.program, spec.args, path)?;
        if !output.success {
            let merged = output.merged_output();
            let detail = if merged.is_empty() {
                output.status_label.clone()
            } else {
                merged
            };
            return Err(ShipShapeError::MechanicExecutionFailed {
                id: self.id.to_string(),
                status: output.status_label,
                detail,
            });
        }
        Ok(true)
    }
}

impl Mechanic for ExternalMechanic {
    fn id(&self) -> &str {
        self.id
    }

    fn audit(&self, path: &Path) -> Result<Vec<Violation>> {
        self.audit_with(path)
    }

    fn dry_run(&self, path: &Path) -> Result<String> {
        self.dry_run_with(path)
    }

    fn apply(&self, path: &Path) -> Result<bool> {
        self.apply_with(path)
    }
}

/// Convert non-empty lines of mechanic command output into [`Violation`] items.
///
/// # Arguments
/// * `id` - Mechanic identifier to attach to each violation.
/// * `output` - Raw stdout/stderr text.
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

/// Mechanic that detects Jupyter-notebook-only repositories and converts them via `lib2notebook2lib`.
#[derive(Clone)]
struct NotebookMechanic {
    /// External converter command wrapper.
    converter: ExternalMechanic,
    /// Filesystem adapter used to inspect repository files.
    fs: Arc<dyn FileSystem + Send + Sync>,
}

impl NotebookMechanic {
    /// Create a [`NotebookMechanic`] with injected [`CommandRunner`] and [`FileSystem`].
    ///
    /// # Arguments
    /// * `runner` - Subprocess runner for `lib2notebook2lib`.
    /// * `fs` - Filesystem implementation for notebook detection.
    fn with_deps(runner: Arc<dyn CommandRunner>, fs: Arc<dyn FileSystem + Send + Sync>) -> Self {
        Self {
            converter: ExternalMechanic {
                id: "lib2nb2lib",
                audit: None,
                dry_run: None,
                apply: Some(CommandSpec {
                    program: "lib2notebook2lib",
                    args: &["--convert"],
                }),
                runner,
            },
            fs,
        }
    }

    /// Detect whether `path` contains `.ipynb` files without Python packaging metadata.
    ///
    /// # Arguments
    /// * `path` - Repository root path.
    fn detect_notebook_only_repo(&self, path: &Path) -> Result<bool> {
        let files = self.fs.list_files(path)?;
        let has_notebooks = files.iter().any(|file| is_notebook_path(file));
        if !has_notebooks {
            return Ok(false);
        }

        if has_python_packaging(&*self.fs, path) {
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

    fn dry_run(&self, path: &Path) -> Result<String> {
        self.converter.dry_run_with(path)
    }

    fn apply(&self, path: &Path) -> Result<bool> {
        self.converter.apply_with(path)
    }
}

/// Mechanic that audits and generates `Dockerfile` and `.gitlab-ci.yml` configurations.
#[derive(Clone)]
struct CiDrydockMechanic {
    /// Filesystem adapter used to check and write CI configuration files.
    fs: Arc<dyn FileSystem + Send + Sync>,
}

impl CiDrydockMechanic {
    /// Create a new [`CiDrydockMechanic`] with the given [`FileSystem`].
    ///
    /// # Arguments
    /// * `fs` - Filesystem implementation.
    fn new(fs: Arc<dyn FileSystem + Send + Sync>) -> Self {
        Self { fs }
    }
}

impl Mechanic for CiDrydockMechanic {
    fn id(&self) -> &str {
        "ci-drydock"
    }

    fn audit(&self, path: &Path) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        if !self.fs.is_file(&path.join("Dockerfile")) {
            violations.push(Violation {
                id: self.id().to_string(),
                message: "Missing Dockerfile at repository root.".to_string(),
            });
        }
        if !self.fs.is_file(&path.join(".gitlab-ci.yml")) {
            violations.push(Violation {
                id: self.id().to_string(),
                message: "Missing .gitlab-ci.yml at repository root.".to_string(),
            });
        }
        Ok(violations)
    }

    fn dry_run(&self, path: &Path) -> Result<String> {
        let (dockerfile, ci) = generate_ci_config_with_fs(&*self.fs, path);
        Ok(format!(
            "# Dockerfile
{dockerfile}
# .gitlab-ci.yml
{ci}"
        ))
    }

    fn apply(&self, path: &Path) -> Result<bool> {
        let (dockerfile, ci) = generate_ci_config_with_fs(&*self.fs, path);
        let mut changed = false;
        let dockerfile_path = path.join("Dockerfile");
        if !self.fs.is_file(&dockerfile_path) {
            self.fs.write_file(&dockerfile_path, &dockerfile)?;
            changed = true;
        }
        let ci_path = path.join(".gitlab-ci.yml");
        if !self.fs.is_file(&ci_path) {
            self.fs.write_file(&ci_path, &ci)?;
            changed = true;
        }
        Ok(changed)
    }
}

/// Construct the `type-correct` external mechanic.
///
/// # Arguments
/// * `runner` - Subprocess runner implementation.
fn type_correct_mechanic(runner: Arc<dyn CommandRunner>) -> ExternalMechanic {
    ExternalMechanic {
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
        runner,
    }
}

/// Construct the `cdd-c` external mechanic.
///
/// # Arguments
/// * `runner` - Subprocess runner implementation.
fn cdd_c_mechanic(runner: Arc<dyn CommandRunner>) -> ExternalMechanic {
    ExternalMechanic {
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
        runner,
    }
}

/// Construct the `go-auto-err-handling` external mechanic.
///
/// # Arguments
/// * `runner` - Subprocess runner implementation.
fn go_auto_err_handling_mechanic(runner: Arc<dyn CommandRunner>) -> ExternalMechanic {
    ExternalMechanic {
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
        runner,
    }
}

/// No-op mechanic implementation for testing and dry runs.
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

#[cfg(test)]
mod tests {
    use super::{
        CommandOutput, CommandRunner, CommandSpec, ExternalMechanic, NotebookMechanic,
        StdCommandRunner, build_mechanics, build_mechanics_with,
    };
    use crate::fs::{FileSystem, StdFileSystem};
    use crate::{Mechanic, Result, ShipShapeError};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Atomic counter for unique test directory names.
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Fake command runner returning a preset [`CommandOutput`].
    struct FakeRunner {
        /// Output returned on each `run` call.
        output: CommandOutput,
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, _program: &str, _args: &[&str], _path: &Path) -> Result<CommandOutput> {
            Ok(self.output.clone())
        }
    }

    #[test]
    fn notebook_audit_reports_violation_when_no_packaging_and_skips_when_none_or_packaged()
    -> Result<()> {
        let root = unique_temp_dir();
        let fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(StdFileSystem::new());
        let runner: Arc<dyn CommandRunner> = Arc::new(FakeRunner {
            output: CommandOutput {
                success: true,
                status_label: "exit status: 0".to_string(),
                stdout: String::new(),
                stderr: String::new(),
            },
        });
        let mechanic = NotebookMechanic::with_deps(Arc::clone(&runner), Arc::clone(&fs));

        // 1. No notebooks at all
        fs.write_file(&root.join("README.md"), "# Demo")?;
        assert!(mechanic.audit(&root)?.is_empty());

        // 2. Notebook without packaging
        fs.write_file(&root.join("analysis.ipynb"), "{}")?;
        let violations = mechanic.audit(&root)?;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].id, "lib2nb2lib");

        // 3. Notebook with packaging
        fs.write_file(
            &root.join("pyproject.toml"),
            r#"[project]
name = "demo""#,
        )?;
        assert!(mechanic.audit(&root)?.is_empty());

        // 4. dry_run returns MechanicNotConfigured, apply delegates to converter
        let dry_err = mechanic.dry_run(&root);
        assert!(matches!(
            dry_err,
            Err(ShipShapeError::MechanicNotConfigured {
                operation: "dry-run",
                ..
            })
        ));
        assert!(mechanic.apply(&root)?);

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn build_mechanics_supports_empty_all_aliases_and_unknown_error() -> Result<()> {
        let empty = build_mechanics(&[])?;
        assert!(empty.is_empty());

        let mechanics = build_mechanics(&[
            "noop".to_string(),
            "lib2notebook2lib".to_string(),
            "notebook-lib".to_string(),
            "cpp-types".to_string(),
            "cdd-c".to_string(),
            "go-err".to_string(),
            "ci-drydock".to_string(),
        ])?;

        let ids: Vec<&str> = mechanics.iter().map(|m| m.id()).collect();
        assert_eq!(
            ids,
            vec![
                "noop",
                "lib2nb2lib",
                "lib2nb2lib",
                "type-correct",
                "cdd-c",
                "go-auto-err-handling",
                "ci-drydock",
            ]
        );

        // Exercise NoopMechanic methods
        let noop = &mechanics[0];
        assert!(noop.audit(Path::new("/tmp"))?.is_empty());
        assert_eq!(noop.dry_run(Path::new("/tmp"))?, "noop");
        assert!(!noop.apply(Path::new("/tmp"))?);

        // Unknown mechanic error
        let unknown = build_mechanics(&["not-a-mechanic".to_string()]);
        assert!(matches!(
            unknown,
            Err(ShipShapeError::UnknownMechanic { ref id }) if id == "not-a-mechanic"
        ));
        Ok(())
    }

    #[test]
    fn ci_drydock_mechanic_audits_dry_runs_and_applies() -> Result<()> {
        let root = unique_temp_dir();
        let fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(StdFileSystem::new());
        fs.write_file(
            &root.join("Cargo.toml"),
            r#"[package]
name = "demo"
"#,
        )?;

        let runner: Arc<dyn CommandRunner> = Arc::new(StdCommandRunner);
        let mechanics = build_mechanics_with(&["ci-drydock".to_string()], &runner, &fs)?;
        let drydock = &mechanics[0];

        let violations = drydock.audit(&root)?;
        assert_eq!(violations.len(), 2);

        let preview = drydock.dry_run(&root)?;
        assert!(preview.contains("FROM rust:1.76"));

        let applied = drydock.apply(&root)?;
        assert!(applied);
        assert!(drydock.audit(&root)?.is_empty());

        // Second apply makes no changes because files already exist
        let applied_again = drydock.apply(&root)?;
        assert!(!applied_again);

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn external_mechanic_handles_all_success_and_error_branches() -> Result<()> {
        let path = Path::new("/tmp");

        // 1. Unconfigured operations
        let unconfigured = ExternalMechanic {
            id: "empty-mech",
            audit: None,
            dry_run: None,
            apply: None,
            runner: Arc::new(FakeRunner {
                output: CommandOutput {
                    success: true,
                    status_label: "0".to_string(),
                    stdout: String::new(),
                    stderr: String::new(),
                },
            }),
        };
        assert!(matches!(
            unconfigured.audit(path),
            Err(ShipShapeError::MechanicNotConfigured {
                operation: "audit",
                ..
            })
        ));
        assert!(matches!(
            unconfigured.dry_run(path),
            Err(ShipShapeError::MechanicNotConfigured {
                operation: "dry-run",
                ..
            })
        ));
        assert!(matches!(
            unconfigured.apply(path),
            Err(ShipShapeError::MechanicNotConfigured {
                operation: "apply",
                ..
            })
        ));

        // 2. Success with both stdout and stderr
        let ok_runner: Arc<dyn CommandRunner> = Arc::new(FakeRunner {
            output: CommandOutput {
                success: true,
                status_label: "exit status: 0".to_string(),
                stdout: "issue one
"
                .to_string(),
                stderr: "issue two
"
                .to_string(),
            },
        });
        let std_fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(StdFileSystem::new());
        let ids = vec![
            "type-correct".to_string(),
            "cdd-c".to_string(),
            "go-auto-err-handling".to_string(),
        ];
        let mechanics = build_mechanics_with(&ids, &ok_runner, &std_fs)?;
        for m in &mechanics {
            let v = m.audit(path)?;
            assert_eq!(v.len(), 2);
            let diff = m.dry_run(path)?;
            assert_eq!(
                diff,
                "issue one
issue two"
            );
            assert!(m.apply(path)?);
        }

        // 3. Non-zero exit with empty output fails audit, dry_run, and apply
        let fail_empty_runner: Arc<dyn CommandRunner> = Arc::new(FakeRunner {
            output: CommandOutput {
                success: false,
                status_label: "exit status: 2".to_string(),
                stdout: String::new(),
                stderr: String::new(),
            },
        });
        let spec = Some(CommandSpec {
            program: "tool",
            args: &[],
        });
        let failing_empty = ExternalMechanic {
            id: "tool",
            audit: spec,
            dry_run: spec,
            apply: spec,
            runner: fail_empty_runner,
        };
        assert!(matches!(
            failing_empty.audit(path),
            Err(ShipShapeError::MechanicExecutionFailed { .. })
        ));
        assert!(matches!(
            failing_empty.dry_run(path),
            Err(ShipShapeError::MechanicExecutionFailed { .. })
        ));
        assert!(matches!(
            failing_empty.apply(path),
            Err(ShipShapeError::MechanicExecutionFailed { ref detail, .. }) if detail == "exit status: 2"
        ));

        // 4. Non-zero exit with diagnostic output succeeds in audit/dry_run (reporting findings) but fails apply with detail
        let fail_diag_runner: Arc<dyn CommandRunner> = Arc::new(FakeRunner {
            output: CommandOutput {
                success: false,
                status_label: "exit status: 1".to_string(),
                stdout: String::new(),
                stderr: "apply error detail".to_string(),
            },
        });
        let failing_diag = ExternalMechanic {
            id: "tool",
            audit: spec,
            dry_run: spec,
            apply: spec,
            runner: fail_diag_runner,
        };
        assert_eq!(failing_diag.audit(path)?.len(), 1);
        assert_eq!(failing_diag.dry_run(path)?, "apply error detail");
        assert!(matches!(
            failing_diag.apply(path),
            Err(ShipShapeError::MechanicExecutionFailed { ref detail, .. }) if detail == "apply error detail"
        ));

        Ok(())
    }

    #[test]
    fn std_command_runner_executes_system_binary() -> Result<()> {
        let runner = StdCommandRunner;
        let output = runner.run("echo", &["shipshape"], Path::new("/tmp"))?;
        assert!(output.success);
        assert!(output.merged_output().contains("shipshape"));
        Ok(())
    }

    /// Generate a unique temporary directory path.
    fn unique_temp_dir() -> PathBuf {
        let pid = std::process::id();
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("shipshape_core_mechanic_test_{pid}_{count}"))
    }
}
