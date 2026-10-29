//! Repository inspection utilities.
//!
//! Provides [`TokeiInspector`] for computing language distributions via `tokei`,
//! [`inspect_coverage`] for evaluating test and documentation ratios, and
//! [`compute_health_score`] for aggregating coverage and violation penalties.

use std::collections::BTreeMap;
use std::path::Path;

use tokei::{Config, LanguageType};

use crate::domain::{CoverageReport, LanguageDistribution, Violation};
use crate::error::Result;
use crate::fs::FileSystem;

/// Inspects a repository using `tokei` to compute language distribution.
pub struct TokeiInspector<F: FileSystem> {
    /// Filesystem adapter used to list and read repository files.
    fs: F,
    /// `tokei` configuration used for language detection and line parsing.
    config: Config,
}

impl<F: FileSystem> TokeiInspector<F> {
    /// Create a new inspector with default `tokei` configuration.
    ///
    /// # Arguments
    /// * `fs` - Filesystem implementation used to traverse and read files.
    pub fn new(fs: F) -> Self {
        Self {
            fs,
            config: Config::default(),
        }
    }

    /// Create a new inspector with a custom `tokei` configuration.
    ///
    /// # Arguments
    /// * `fs` - Filesystem implementation used to traverse and read files.
    /// * `config` - Custom `tokei::Config` settings.
    pub fn with_config(fs: F, config: Config) -> Self {
        Self { fs, config }
    }

    /// Inspect the repository at `root` and return language distribution percentages.
    ///
    /// Uses `tokei::LanguageType::parse_from_str` to count code lines (falling back to
    /// non-empty line counts when a language grammar only tracks plain lines).
    ///
    /// # Arguments
    /// * `root` - Root directory of the repository to inspect.
    pub fn inspect(&self, root: &Path) -> Result<LanguageDistribution> {
        let files = self.fs.list_files(root)?;
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut total = 0usize;

        for path in files {
            let Some(language) = LanguageType::from_path(&path, &self.config) else {
                continue;
            };
            let contents = self.fs.read_to_string(&path)?;
            let lines = count_language_lines(language, &contents, &self.config);
            if lines == 0 {
                continue;
            }
            total += lines;
            let key = language.to_string();
            *counts.entry(key).or_insert(0) += lines;
        }

        if total == 0 {
            return Ok(BTreeMap::new());
        }

        let mut distribution = BTreeMap::new();
        for (language, count) in counts {
            let percentage = (count as f64 / total as f64) * 100.0;
            distribution.insert(language, percentage);
        }

        Ok(distribution)
    }
}

/// Inspect a repository for test and documentation coverage heuristics.
///
/// # Arguments
/// * `fs` - Filesystem implementation used to list repository files.
/// * `root` - Root directory of the repository to inspect.
pub fn inspect_coverage<F: FileSystem>(fs: &F, root: &Path) -> Result<CoverageReport> {
    let files = fs.list_files(root)?;
    let mut code_files = 0usize;
    let mut test_files = 0usize;
    let mut doc_files = 0usize;

    for path in files {
        if is_doc_file(&path) {
            doc_files += 1;
        }
        if is_code_file(&path) {
            code_files += 1;
            if is_test_file(&path) {
                test_files += 1;
            }
        }
    }

    let (test_coverage, doc_coverage, low_test_coverage, low_doc_coverage) =
        compute_coverage_metrics(code_files, test_files, doc_files);

    Ok(CoverageReport {
        code_files,
        test_files,
        doc_files,
        test_coverage,
        doc_coverage,
        low_test_coverage,
        low_doc_coverage,
    })
}

/// Compute a heuristic health score (`0..=100`) using coverage metrics and violations.
///
/// # Arguments
/// * `coverage` - Repository coverage report.
/// * `violations` - Slice of audit violations discovered.
#[must_use]
pub fn compute_health_score(coverage: &CoverageReport, violations: &[Violation]) -> u8 {
    let mut score: i32 = 100;
    score -= test_coverage_penalty(coverage);
    score -= doc_coverage_penalty(coverage);
    score -= violation_penalty(violations.len());

    score.clamp(0, 100) as u8
}

/// Calculate test and documentation coverage ratios and low-coverage flags.
///
/// # Arguments
/// * `code_files` - Total number of code files.
/// * `test_files` - Total number of test files.
/// * `doc_files` - Total number of documentation files.
fn compute_coverage_metrics(
    code_files: usize,
    test_files: usize,
    doc_files: usize,
) -> (f64, f64, bool, bool) {
    if code_files == 0 {
        return (0.0, 0.0, false, false);
    }

    let test_coverage = (test_files as f64 / code_files as f64).min(1.0);
    let doc_coverage = (doc_files as f64 / code_files as f64).min(1.0);

    let low_test_coverage = test_coverage < 0.20;
    let low_doc_coverage = doc_coverage < 0.10;

    (
        test_coverage,
        doc_coverage,
        low_test_coverage,
        low_doc_coverage,
    )
}

/// Compute the health score penalty (`0..=35`) for test coverage.
///
/// # Arguments
/// * `coverage` - Coverage metrics to evaluate.
fn test_coverage_penalty(coverage: &CoverageReport) -> i32 {
    if coverage.code_files == 0 {
        return 0;
    }

    if coverage.test_coverage < 0.10 {
        35
    } else if coverage.test_coverage < 0.20 {
        25
    } else if coverage.test_coverage < 0.40 {
        10
    } else {
        0
    }
}

/// Compute the health score penalty (`0..=25`) for documentation coverage.
///
/// # Arguments
/// * `coverage` - Coverage metrics to evaluate.
fn doc_coverage_penalty(coverage: &CoverageReport) -> i32 {
    if coverage.code_files == 0 {
        return 0;
    }

    if coverage.doc_coverage < 0.05 {
        25
    } else if coverage.doc_coverage < 0.10 {
        15
    } else if coverage.doc_coverage < 0.20 {
        5
    } else {
        0
    }
}

/// Compute the health score penalty (`0..=30`) for violation count.
///
/// # Arguments
/// * `violations` - Number of violations discovered.
fn violation_penalty(violations: usize) -> i32 {
    let penalty = i32::try_from(violations).unwrap_or(30).saturating_mul(2);
    penalty.min(30)
}

/// Count code lines for `language` in `text` using `tokei`.
///
/// # Arguments
/// * `language` - Detected `tokei::LanguageType`.
/// * `text` - File contents.
/// * `config` - `tokei::Config` options.
fn count_language_lines(language: LanguageType, text: &str, config: &Config) -> usize {
    let stats = language.parse_from_str(text, config);
    if stats.code > 0 {
        stats.code
    } else {
        text.lines().filter(|line| !line.trim().is_empty()).count()
    }
}

/// Determine whether `path` has a recognized source-code extension.
///
/// # Arguments
/// * `path` - File path to inspect.
fn is_code_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase);
    let Some(ext) = ext else {
        return false;
    };
    matches!(
        ext.as_str(),
        "rs" | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "go"
            | "java"
            | "kt"
            | "kts"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cc"
            | "cxx"
            | "cs"
            | "rb"
            | "php"
            | "swift"
    )
}

/// Determine whether `path` is a documentation file by filename, directory, or extension.
///
/// # Arguments
/// * `path` - File path to inspect.
fn is_doc_file(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    if file_name == "readme" || file_name.starts_with("readme.") {
        return true;
    }

    if path_components_match(path, &["docs", "documentation"]) {
        return true;
    }

    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();

    matches!(ext.as_str(), "md" | "mdx" | "rst" | "adoc" | "txt")
}

/// Determine whether `path` is a test or specification file.
///
/// # Arguments
/// * `path` - File path to inspect.
fn is_test_file(path: &Path) -> bool {
    if path_components_match(path, &["test", "tests", "spec", "specs"]) {
        return true;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    if file_name.contains(".test.") || file_name.contains(".spec.") {
        return true;
    }

    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.starts_with("spec_")
        || stem.ends_with("_spec")
}

/// Return `true` if any component in `path` matches one of `segments` (case-insensitive).
///
/// # Arguments
/// * `path` - Path whose components will be checked.
/// * `segments` - Lowercase segment names to match against.
fn path_components_match(path: &Path, segments: &[&str]) -> bool {
    path.components().any(|component| {
        let segment = component.as_os_str().to_string_lossy().to_lowercase();
        segments.contains(&segment.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CoverageReport, TokeiInspector, Violation, compute_health_score, inspect_coverage,
    };
    use crate::error::Result;
    use crate::fs::MockFileSystem;
    use std::path::{Path, PathBuf};
    use tokei::Config;

    #[test]
    fn inspect_reports_language_distribution_and_skips_unknown_extensions() -> Result<()> {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files().returning(|_| {
            Ok(vec![
                PathBuf::from("src/main.rs"),
                PathBuf::from("src/app.py"),
                PathBuf::from("assets/logo.unknown_ext"),
            ])
        });
        fs.expect_read_to_string()
            .withf(|path| path == Path::new("src/main.rs"))
            .returning(|_| {
                Ok("// comment
fn main() {}
"
                .to_string())
            });
        fs.expect_read_to_string()
            .withf(|path| path == Path::new("src/app.py"))
            .returning(|_| {
                Ok("print('hi')
"
                .to_string())
            });

        let inspector = TokeiInspector::with_config(fs, Config::default());
        let distribution = inspector.inspect(Path::new("/repo"))?;

        let rust_key = tokei::LanguageType::Rust.to_string();
        let python_key = tokei::LanguageType::Python.to_string();

        assert_eq!(distribution.get(&rust_key).copied(), Some(50.0));
        assert_eq!(distribution.get(&python_key).copied(), Some(50.0));
        Ok(())
    }

    #[test]
    fn inspect_falls_back_to_non_empty_lines_for_comment_or_markdown_files() -> Result<()> {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files().returning(|_| {
            Ok(vec![
                PathBuf::from("README.md"),
                PathBuf::from("src/empty.rs"),
            ])
        });
        fs.expect_read_to_string()
            .withf(|path| path == Path::new("README.md"))
            .returning(|_| {
                Ok("# Title

Paragraph
"
                .to_string())
            });
        fs.expect_read_to_string()
            .withf(|path| path == Path::new("src/empty.rs"))
            .returning(|_| {
                Ok("   

"
                .to_string())
            });

        let inspector = TokeiInspector::new(fs);
        let distribution = inspector.inspect(Path::new("/repo"))?;
        let md_key = tokei::LanguageType::Markdown.to_string();
        assert_eq!(distribution.get(&md_key).copied(), Some(100.0));
        Ok(())
    }

    #[test]
    fn inspect_returns_empty_when_no_lines_found() -> Result<()> {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files()
            .returning(|_| Ok(vec![PathBuf::from("src/empty.rs")]));
        fs.expect_read_to_string().returning(|_| Ok(String::new()));

        let inspector = TokeiInspector::new(fs);
        let distribution = inspector.inspect(Path::new("/repo"))?;

        assert!(distribution.is_empty());
        Ok(())
    }

    #[test]
    fn coverage_handles_empty_repository() -> Result<()> {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files().returning(|_| Ok(Vec::new()));

        let coverage = inspect_coverage(&fs, Path::new("/repo"))?;
        assert_eq!(coverage.code_files, 0);
        assert_eq!(coverage.test_files, 0);
        assert_eq!(coverage.doc_files, 0);
        assert!((coverage.test_coverage - 0.0).abs() < f64::EPSILON);
        assert!((coverage.doc_coverage - 0.0).abs() < f64::EPSILON);
        assert!(!coverage.low_test_coverage);
        assert!(!coverage.low_doc_coverage);
        assert_eq!(compute_health_score(&coverage, &[]), 100);
        Ok(())
    }

    #[test]
    fn coverage_flags_low_when_tests_and_docs_missing() -> Result<()> {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files().returning(|_| {
            Ok(vec![
                PathBuf::from("src/main.rs"),
                PathBuf::from("src/lib.rs"),
                PathBuf::from("Makefile"),
            ])
        });

        let coverage = inspect_coverage(&fs, Path::new("/repo"))?;

        assert_eq!(coverage.code_files, 2);
        assert_eq!(coverage.test_files, 0);
        assert_eq!(coverage.doc_files, 0);
        assert!(coverage.low_test_coverage);
        assert!(coverage.low_doc_coverage);
        Ok(())
    }

    #[test]
    fn coverage_recognizes_all_doc_and_test_file_patterns() -> Result<()> {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files().returning(|_| {
            Ok(vec![
                PathBuf::from("src/main.rs"),
                PathBuf::from("src/widget.test.ts"),
                PathBuf::from("src/widget.spec.ts"),
                PathBuf::from("src/test_helper.py"),
                PathBuf::from("src/helper_test.go"),
                PathBuf::from("src/spec_runner.rb"),
                PathBuf::from("src/runner_spec.rb"),
                PathBuf::from("tests/integration.rs"),
                PathBuf::from("README"),
                PathBuf::from("README.md"),
                PathBuf::from("docs/guide.BizarreExt"),
                PathBuf::from("notes.rst"),
            ])
        });

        let coverage = inspect_coverage(&fs, Path::new("/repo"))?;

        assert_eq!(coverage.code_files, 8);
        assert_eq!(coverage.test_files, 7);
        assert_eq!(coverage.doc_files, 4);
        assert!(!coverage.low_test_coverage);
        assert!(!coverage.low_doc_coverage);
        Ok(())
    }

    #[test]
    fn health_score_evaluates_exact_penalty_tiers() {
        // Tier 1: test_coverage < 0.10 (-35), doc_coverage < 0.05 (-25), 2 violations (-4) => 36
        let tier1 = CoverageReport {
            code_files: 20,
            test_files: 1,
            doc_files: 0,
            test_coverage: 0.05,
            doc_coverage: 0.0,
            low_test_coverage: true,
            low_doc_coverage: true,
        };
        let violations = vec![
            Violation {
                id: "v1".to_string(),
                message: "first".to_string(),
            },
            Violation {
                id: "v2".to_string(),
                message: "second".to_string(),
            },
        ];
        assert_eq!(compute_health_score(&tier1, &violations), 36);

        // Tier 2: 0.10 <= test_coverage < 0.20 (-25), 0.05 <= doc_coverage < 0.10 (-15), 0 violations => 60
        let tier2 = CoverageReport {
            code_files: 20,
            test_files: 2,
            doc_files: 1,
            test_coverage: 0.10,
            doc_coverage: 0.05,
            low_test_coverage: true,
            low_doc_coverage: true,
        };
        assert_eq!(compute_health_score(&tier2, &[]), 60);

        // Tier 3: 0.20 <= test_coverage < 0.40 (-10), 0.10 <= doc_coverage < 0.20 (-5), 20 violations (capped at -30) => 55
        let tier3 = CoverageReport {
            code_files: 10,
            test_files: 2,
            doc_files: 1,
            test_coverage: 0.20,
            doc_coverage: 0.10,
            low_test_coverage: false,
            low_doc_coverage: false,
        };
        let many_violations: Vec<Violation> = (0..20)
            .map(|i| Violation {
                id: format!("v{i}"),
                message: "issue".to_string(),
            })
            .collect();
        assert_eq!(compute_health_score(&tier3, &many_violations), 55);

        // Tier 4: test_coverage >= 0.40 (0), doc_coverage >= 0.20 (0), 0 violations => 100
        let tier4 = CoverageReport {
            code_files: 10,
            test_files: 4,
            doc_files: 2,
            test_coverage: 0.40,
            doc_coverage: 0.20,
            low_test_coverage: false,
            low_doc_coverage: false,
        };
        assert_eq!(compute_health_score(&tier4, &[]), 100);
    }
}
