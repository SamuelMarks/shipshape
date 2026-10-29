//! PR template interpolation for ShipShape placeholders.
//!
//! Locates `PULL_REQUEST_TEMPLATE.md` files via [`FileSystem`], validates placeholder
//! presence, and interpolates [`PrTemplateContext`] metrics derived from [`FleetReport`].

use std::path::{Path, PathBuf};

use crate::ShipShapeError;
use crate::domain::{FleetReport, LanguageDistribution};
use crate::error::Result;
use crate::fs::{FileSystem, StdFileSystem};
use crate::report::format_language_stats;

/// Placeholder token for ShipShape statistics.
pub const SHIPSHAPE_STATS: &str = "{{SHIPSHAPE_STATS}}";
/// Placeholder token for ShipShape fixes summary.
pub const SHIPSHAPE_FIXES: &str = "{{SHIPSHAPE_FIXES}}";
/// Placeholder token for ShipShape CI configuration.
pub const SHIPSHAPE_CI: &str = "{{SHIPSHAPE_CI}}";

/// Interpolated PR template values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrTemplateContext {
    /// Text replacement for stats placeholder.
    pub stats: String,
    /// Text replacement for fixes placeholder.
    pub fixes: String,
    /// Text replacement for CI placeholder.
    pub ci: String,
}

impl PrTemplateContext {
    /// Build a template context from a ShipShape fleet report.
    ///
    /// # Arguments
    /// * `report` - Fleet health and coverage report.
    #[must_use]
    pub fn from_report(report: &FleetReport) -> Self {
        Self {
            stats: format_stats(report),
            fixes: format_fixes(report),
            ci: format_ci(report),
        }
    }
}

/// Attempt to locate and interpolate a PR template for the given repository root.
///
/// # Arguments
/// * `fs` - Filesystem implementation used to locate and read the template.
/// * `repo_root` - Repository root directory.
/// * `context` - Replacement values for `{{SHIPSHAPE_*}}` placeholders.
pub fn interpolate_pr_template<F: FileSystem + ?Sized>(
    fs: &F,
    repo_root: &Path,
    context: &PrTemplateContext,
) -> Result<Option<String>> {
    let Some(template_path) = find_pr_template_with_fs(fs, repo_root) else {
        return Ok(None);
    };

    let template = fs.read_to_string(&template_path)?;
    let rendered = apply_context(template, context);
    Ok(Some(rendered))
}

/// Locate a PR template in the repository using [`StdFileSystem`], if it exists.
///
/// # Arguments
/// * `repo_root` - Repository root directory.
#[must_use]
pub fn find_pr_template(repo_root: &Path) -> Option<PathBuf> {
    let fs = StdFileSystem::new();
    find_pr_template_with_fs(&fs, repo_root)
}

/// Locate a PR template in the repository using the provided [`FileSystem`], if it exists.
///
/// Checks `PULL_REQUEST_TEMPLATE.md` at the root first, then `.github/PULL_REQUEST_TEMPLATE.md`.
///
/// # Arguments
/// * `fs` - Filesystem implementation.
/// * `repo_root` - Repository root directory.
pub fn find_pr_template_with_fs<F: FileSystem + ?Sized>(
    fs: &F,
    repo_root: &Path,
) -> Option<PathBuf> {
    let candidates = [
        repo_root.join("PULL_REQUEST_TEMPLATE.md"),
        repo_root.join(".github").join("PULL_REQUEST_TEMPLATE.md"),
    ];
    candidates.into_iter().find(|path| fs.is_file(path))
}

/// Format the statistics block for `{{SHIPSHAPE_STATS}}`.
///
/// # Arguments
/// * `report` - Fleet report containing health, coverage, and language stats.
fn format_stats(report: &FleetReport) -> String {
    let coverage = &report.coverage;
    let languages = format_languages(&report.language_stats);
    [
        "ShipShape stats:".to_string(),
        format!("- Health score: {}/100", report.health_score),
        format!(
            "- Test coverage: {:.1}% ({}/{})",
            coverage.test_coverage * 100.0,
            coverage.test_files,
            coverage.code_files
        ),
        format!(
            "- Doc coverage: {:.1}% ({}/{})",
            coverage.doc_coverage * 100.0,
            coverage.doc_files,
            coverage.code_files
        ),
        format!("- Languages: {languages}"),
    ]
    .join(
        "
",
    )
}

/// Format the violations summary for `{{SHIPSHAPE_FIXES}}`.
///
/// # Arguments
/// * `report` - Fleet report containing audit violations.
fn format_fixes(report: &FleetReport) -> String {
    if report.violations.is_empty() {
        return "No violations detected.".to_string();
    }
    let mut lines = Vec::with_capacity(report.violations.len() + 1);
    lines.push("Violations:".to_string());
    for violation in &report.violations {
        lines.push(format!("- {} ({})", violation.message, violation.id));
    }
    lines.join(
        "
",
    )
}

/// Format the coverage gate summary for `{{SHIPSHAPE_CI}}`.
///
/// # Arguments
/// * `report` - Fleet report containing coverage gate statuses.
fn format_ci(report: &FleetReport) -> String {
    let coverage = &report.coverage;
    [
        "Coverage gates:".to_string(),
        format!(
            "- Tests: {} ({:.1}%, {}/{})",
            coverage_status(coverage.low_test_coverage),
            coverage.test_coverage * 100.0,
            coverage.test_files,
            coverage.code_files
        ),
        format!(
            "- Docs: {} ({:.1}%, {}/{})",
            coverage_status(coverage.low_doc_coverage),
            coverage.doc_coverage * 100.0,
            coverage.doc_files,
            coverage.code_files
        ),
    ]
    .join(
        "
",
    )
}

/// Return `"low"` when `low` is `true`, otherwise `"ok"`.
///
/// # Arguments
/// * `low` - Flag indicating whether coverage is below the threshold.
const fn coverage_status(low: bool) -> &'static str {
    if low { "low" } else { "ok" }
}

/// Format language distribution map as a comma-separated list sorted by percentage.
///
/// # Arguments
/// * `stats` - Language distribution map.
fn format_languages(stats: &LanguageDistribution) -> String {
    if stats.is_empty() {
        return "No languages detected.".to_string();
    }
    format_language_stats(stats)
        .into_iter()
        .map(|(language, percent)| format!("{language} {percent:.2}%"))
        .collect::<Vec<String>>()
        .join(", ")
}

/// Replace `{{SHIPSHAPE_*}}` placeholders in `template` with values from `context`.
///
/// # Arguments
/// * `template` - Raw PR template string.
/// * `context` - Replacement strings.
fn apply_context(mut template: String, context: &PrTemplateContext) -> String {
    if template.contains(SHIPSHAPE_STATS) {
        template = template.replace(SHIPSHAPE_STATS, context.stats.trim());
    }
    if template.contains(SHIPSHAPE_FIXES) {
        template = template.replace(SHIPSHAPE_FIXES, context.fixes.trim());
    }
    if template.contains(SHIPSHAPE_CI) {
        template = template.replace(SHIPSHAPE_CI, context.ci.trim());
    }
    template
}

/// Ensure all required ShipShape placeholders are present in `template`.
///
/// # Arguments
/// * `template` - Template content to validate.
pub fn ensure_placeholders(template: &str) -> Result<()> {
    let missing: Vec<&'static str> = [SHIPSHAPE_STATS, SHIPSHAPE_FIXES, SHIPSHAPE_CI]
        .into_iter()
        .filter(|token| !template.contains(*token))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(ShipShapeError::MissingPrPlaceholders { missing })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFileSystem;
    use crate::{CoverageReport, FleetReport, Violation};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counter for unique test directories.
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn interpolate_replaces_all_placeholders_and_handles_no_placeholders() {
        let template = format!(
            "{SHIPSHAPE_STATS}
{SHIPSHAPE_FIXES}
{SHIPSHAPE_CI}
"
        );
        let context = PrTemplateContext {
            stats: "stats".to_string(),
            fixes: "fixes".to_string(),
            ci: "ci".to_string(),
        };

        let rendered = apply_context(template, &context);
        assert!(rendered.contains("stats"));
        assert!(rendered.contains("fixes"));
        assert!(rendered.contains("ci"));

        let plain = apply_context("plain text".to_string(), &context);
        assert_eq!(plain, "plain text");
    }

    #[test]
    fn ensure_placeholders_accepts_complete_and_reports_missing() -> Result<()> {
        let complete = format!("{SHIPSHAPE_STATS} {SHIPSHAPE_FIXES} {SHIPSHAPE_CI}");
        ensure_placeholders(&complete)?;

        let result = ensure_placeholders("{{SHIPSHAPE_STATS}}");
        assert!(matches!(
            result,
            Err(ShipShapeError::MissingPrPlaceholders { .. })
        ));
        Ok(())
    }

    #[test]
    fn find_pr_template_prefers_repo_root_and_returns_none_when_absent() -> Result<()> {
        let root = temp_dir_with_template("PULL_REQUEST_TEMPLATE.md")?;
        let found = find_pr_template(&root);
        assert_eq!(found, Some(root.join("PULL_REQUEST_TEMPLATE.md")));
        std::fs::remove_dir_all(&root)?;

        let mut fs = MockFileSystem::new();
        fs.expect_is_file().returning(|_| false);
        let context = PrTemplateContext {
            stats: "stats".to_string(),
            fixes: "fixes".to_string(),
            ci: "ci".to_string(),
        };
        let rendered = interpolate_pr_template(&fs, Path::new("/empty"), &context)?;
        assert_eq!(rendered, None);
        Ok(())
    }

    #[test]
    fn interpolate_reads_and_renders_template() -> Result<()> {
        let root = PathBuf::from("/repo");
        let root_candidate = root.join("PULL_REQUEST_TEMPLATE.md");
        let github_candidate = root.join(".github").join("PULL_REQUEST_TEMPLATE.md");

        let mut fs = MockFileSystem::new();
        fs.expect_is_file()
            .withf(move |path| path == root_candidate)
            .returning(|_| false);
        let expected_read = github_candidate.clone();
        fs.expect_is_file()
            .withf(move |path| path == github_candidate)
            .returning(|_| true);
        fs.expect_read_to_string()
            .withf(move |path| path == expected_read)
            .returning(|_| Ok("{{SHIPSHAPE_STATS}}".to_string()));

        let context = PrTemplateContext {
            stats: "stats".to_string(),
            fixes: "fixes".to_string(),
            ci: "ci".to_string(),
        };

        let rendered = interpolate_pr_template(&fs, &root, &context)?;
        assert_eq!(rendered, Some("stats".to_string()));
        Ok(())
    }

    #[test]
    fn context_from_report_formats_sections() {
        let mut language_stats = BTreeMap::new();
        language_stats.insert("Rust".to_string(), 70.0);
        language_stats.insert("Go".to_string(), 30.0);
        let report = FleetReport {
            language_stats,
            violations: vec![Violation {
                id: "doc-1".to_string(),
                message: "Missing docs".to_string(),
            }],
            coverage: CoverageReport {
                code_files: 10,
                test_files: 5,
                doc_files: 2,
                test_coverage: 0.5,
                doc_coverage: 0.2,
                low_test_coverage: false,
                low_doc_coverage: true,
            },
            health_score: 84,
        };

        let context = PrTemplateContext::from_report(&report);

        assert!(context.stats.contains("Health score: 84/100"));
        assert!(context.stats.contains("Test coverage: 50.0% (5/10)"));
        assert!(context.stats.contains("Doc coverage: 20.0% (2/10)"));
        assert!(context.stats.contains("Languages: Rust 70.00%, Go 30.00%"));
        assert!(context.fixes.contains("Violations:"));
        assert!(context.fixes.contains("Missing docs (doc-1)"));
        assert!(context.ci.contains("Coverage gates:"));
        assert!(context.ci.contains("Tests: ok (50.0%"));
        assert!(context.ci.contains("Docs: low (20.0%"));
    }

    #[test]
    fn context_from_report_handles_empty_sections() {
        let report = FleetReport {
            language_stats: BTreeMap::new(),
            violations: Vec::new(),
            coverage: CoverageReport {
                code_files: 0,
                test_files: 0,
                doc_files: 0,
                test_coverage: 0.0,
                doc_coverage: 0.0,
                low_test_coverage: true,
                low_doc_coverage: true,
            },
            health_score: 0,
        };

        let context = PrTemplateContext::from_report(&report);

        assert!(context.stats.contains("Languages: No languages detected."));
        assert_eq!(context.fixes, "No violations detected.");
        assert!(context.ci.contains("Tests: low"));
        assert!(context.ci.contains("Docs: low"));
    }

    /// Create a temporary directory containing a template at `rel_path`.
    fn temp_dir_with_template(rel_path: &str) -> Result<PathBuf> {
        let pid = std::process::id();
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("shipshape_pr_template_test_{pid}_{count}"));
        let template_path = root.join(rel_path);
        StdFileSystem::new().write_file(&template_path, "placeholder")?;
        Ok(root)
    }
}
