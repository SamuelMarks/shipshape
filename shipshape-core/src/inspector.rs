//! Repository inspection utilities.

use std::collections::BTreeMap;
use std::path::Path;

use tokei::{Config, LanguageType};

use crate::domain::{CoverageReport, LanguageDistribution, Violation};
use crate::error::Result;
use crate::fs::FileSystem;

/// Inspects a repository using `tokei` to compute language distribution.
pub struct TokeiInspector<F: FileSystem> {
    fs: F,
    config: Config,
}

impl<F: FileSystem> TokeiInspector<F> {
    /// Create a new inspector with default `tokei` configuration.
    pub fn new(fs: F) -> Self {
        Self {
            fs,
            config: Config::default(),
        }
    }

    /// Create a new inspector with a custom `tokei` configuration.
    pub fn with_config(fs: F, config: Config) -> Self {
        Self { fs, config }
    }

    /// Inspect the repository and return language distribution percentages.
    pub fn inspect(&self, root: &Path) -> Result<LanguageDistribution> {
        let files = self.fs.list_files(root)?;
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut total = 0usize;

        for path in files {
            let Some(language) = LanguageType::from_path(&path, &self.config) else {
                continue;
            };
            let contents = self.fs.read_to_string(&path)?;
            let lines = count_lines(&contents);
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

/// Compute a heuristic health score using coverage metrics and violations.
pub fn compute_health_score(coverage: &CoverageReport, violations: &[Violation]) -> u8 {
    let mut score: i32 = 100;
    score -= test_coverage_penalty(coverage);
    score -= doc_coverage_penalty(coverage);
    score -= violation_penalty(violations.len());

    score.clamp(0, 100) as u8
}

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

fn violation_penalty(violations: usize) -> i32 {
    let penalty = violations as i32 * 2;
    penalty.min(30)
}

fn count_lines(text: &str) -> usize {
    text.lines().count()
}

fn is_code_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());
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

fn is_doc_file(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_lowercase())
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
        .map(|ext| ext.to_lowercase())
        .unwrap_or_default();

    matches!(ext.as_str(), "md" | "mdx" | "rst" | "adoc" | "txt")
}

fn is_test_file(path: &Path) -> bool {
    if path_components_match(path, &["test", "tests", "spec", "specs"]) {
        return true;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_lowercase())
        .unwrap_or_default();
    if file_name.contains(".test.") || file_name.contains(".spec.") {
        return true;
    }

    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(|name| name.to_lowercase())
        .unwrap_or_default();
    stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.starts_with("spec_")
        || stem.ends_with("_spec")
}

fn path_components_match(path: &Path, segments: &[&str]) -> bool {
    path.components().any(|component| {
        let segment = component.as_os_str().to_string_lossy().to_lowercase();
        segments.iter().any(|target| *target == segment)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CoverageReport, TokeiInspector, Violation, compute_health_score, inspect_coverage,
    };
    use crate::fs::MockFileSystem;
    use std::path::{Path, PathBuf};

    #[test]
    fn inspect_reports_language_distribution() {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files().returning(|_| {
            Ok(vec![
                PathBuf::from("src/main.rs"),
                PathBuf::from("src/app.py"),
            ])
        });
        fs.expect_read_to_string()
            .withf(|path| path == Path::new("src/main.rs"))
            .returning(|_| Ok("fn main() {}\n".to_string()));
        fs.expect_read_to_string()
            .withf(|path| path == Path::new("src/app.py"))
            .returning(|_| Ok("print('hi')\n".to_string()));

        let inspector = TokeiInspector::new(fs);
        let distribution = inspector
            .inspect(Path::new("/repo"))
            .expect("inspect succeeds");

        let rust_key = tokei::LanguageType::Rust.to_string();
        let python_key = tokei::LanguageType::Python.to_string();

        assert_eq!(distribution.get(&rust_key).copied(), Some(50.0));
        assert_eq!(distribution.get(&python_key).copied(), Some(50.0));
    }

    #[test]
    fn inspect_returns_empty_when_no_lines_found() {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files()
            .returning(|_| Ok(vec![PathBuf::from("src/empty.rs")]));
        fs.expect_read_to_string().returning(|_| Ok(String::new()));

        let inspector = TokeiInspector::new(fs);
        let distribution = inspector
            .inspect(Path::new("/repo"))
            .expect("inspect succeeds");

        assert!(distribution.is_empty());
    }

    #[test]
    fn coverage_flags_low_when_tests_and_docs_missing() {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files().returning(|_| {
            Ok(vec![
                PathBuf::from("src/main.rs"),
                PathBuf::from("src/lib.rs"),
            ])
        });

        let coverage = inspect_coverage(&fs, Path::new("/repo")).expect("coverage");

        assert_eq!(coverage.code_files, 2);
        assert_eq!(coverage.test_files, 0);
        assert_eq!(coverage.doc_files, 0);
        assert!(coverage.low_test_coverage);
        assert!(coverage.low_doc_coverage);
    }

    #[test]
    fn coverage_counts_tests_and_docs() {
        let mut fs = MockFileSystem::new();
        fs.expect_list_files().returning(|_| {
            Ok(vec![
                PathBuf::from("src/main.rs"),
                PathBuf::from("tests/main_test.rs"),
                PathBuf::from("README.md"),
                PathBuf::from("docs/overview.md"),
            ])
        });

        let coverage = inspect_coverage(&fs, Path::new("/repo")).expect("coverage");

        assert_eq!(coverage.code_files, 2);
        assert_eq!(coverage.test_files, 1);
        assert_eq!(coverage.doc_files, 2);
        assert!(!coverage.low_test_coverage);
        assert!(!coverage.low_doc_coverage);
    }

    #[test]
    fn health_score_accounts_for_violations_and_coverage() {
        let coverage = CoverageReport {
            code_files: 10,
            test_files: 1,
            doc_files: 0,
            test_coverage: 0.10,
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

        let score = compute_health_score(&coverage, &violations);

        assert!(score < 100);
    }
}
