//! PR template interpolation for ShipShape placeholders.

use crate::ShipShapeError;
use crate::domain::{FleetReport, LanguageDistribution};
use crate::error::Result;
use crate::fs::FileSystem;
use crate::report::format_language_stats;
use std::fmt::Write;
use std::path::{Path, PathBuf};

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
    pub fn from_report(report: &FleetReport) -> Self {
        Self {
            stats: format_stats(report),
            fixes: format_fixes(report),
            ci: format_ci(report),
        }
    }
}

/// Attempt to locate and interpolate a PR template for the given repository root.
pub fn interpolate_pr_template<F: FileSystem>(
    fs: &F,
    repo_root: &Path,
    context: &PrTemplateContext,
) -> Result<Option<String>> {
    let template_path = find_pr_template(repo_root);
    let Some(template_path) = template_path else {
        return Ok(None);
    };

    let template = fs.read_to_string(&template_path)?;
    let rendered = apply_context(template, context);
    Ok(Some(rendered))
}

/// Locate a PR template in the repository, if it exists.
pub fn find_pr_template(repo_root: &Path) -> Option<PathBuf> {
    let candidates = [
        repo_root.join("PULL_REQUEST_TEMPLATE.md"),
        repo_root.join(".github").join("PULL_REQUEST_TEMPLATE.md"),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

fn format_stats(report: &FleetReport) -> String {
    let mut output = String::new();
    let coverage = &report.coverage;
    let _ = writeln!(output, "ShipShape stats:");
    let _ = writeln!(output, "- Health score: {}/100", report.health_score);
    let _ = writeln!(
        output,
        "- Test coverage: {:.1}% ({}/{})",
        coverage.test_coverage * 100.0,
        coverage.test_files,
        coverage.code_files
    );
    let _ = writeln!(
        output,
        "- Doc coverage: {:.1}% ({}/{})",
        coverage.doc_coverage * 100.0,
        coverage.doc_files,
        coverage.code_files
    );
    let languages = format_languages(&report.language_stats);
    let _ = writeln!(output, "- Languages: {languages}");
    output.trim_end().to_string()
}

fn format_fixes(report: &FleetReport) -> String {
    if report.violations.is_empty() {
        return "No violations detected.".to_string();
    }
    let mut output = String::new();
    let _ = writeln!(output, "Violations:");
    for violation in &report.violations {
        let _ = writeln!(output, "- {} ({})", violation.message, violation.id);
    }
    output.trim_end().to_string()
}

fn format_ci(report: &FleetReport) -> String {
    let coverage = &report.coverage;
    let mut output = String::new();
    let _ = writeln!(output, "Coverage gates:");
    let _ = writeln!(
        output,
        "- Tests: {} ({:.1}%, {}/{})",
        coverage_status(coverage.low_test_coverage),
        coverage.test_coverage * 100.0,
        coverage.test_files,
        coverage.code_files
    );
    let _ = writeln!(
        output,
        "- Docs: {} ({:.1}%, {}/{})",
        coverage_status(coverage.low_doc_coverage),
        coverage.doc_coverage * 100.0,
        coverage.doc_files,
        coverage.code_files
    );
    output.trim_end().to_string()
}

fn coverage_status(low: bool) -> &'static str {
    if low { "low" } else { "ok" }
}

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

/// Helper to ensure placeholders are present in a template, returning an error if not.
pub fn ensure_placeholders(template: &str) -> Result<()> {
    let missing: Vec<&str> = [SHIPSHAPE_STATS, SHIPSHAPE_FIXES, SHIPSHAPE_CI]
        .into_iter()
        .filter(|token| !template.contains(*token))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(ShipShapeError::Other(format!(
            "missing placeholders: {}",
            missing.join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFileSystem;
    use crate::{CoverageReport, FleetReport, Violation};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn interpolate_replaces_all_placeholders() {
        let template = format!("{SHIPSHAPE_STATS}\n{SHIPSHAPE_FIXES}\n{SHIPSHAPE_CI}\n");
        let context = PrTemplateContext {
            stats: "stats".to_string(),
            fixes: "fixes".to_string(),
            ci: "ci".to_string(),
        };

        let rendered = apply_context(template, &context);

        assert!(rendered.contains("stats"));
        assert!(rendered.contains("fixes"));
        assert!(rendered.contains("ci"));
    }

    #[test]
    fn ensure_placeholders_reports_missing() {
        let result = ensure_placeholders("{{SHIPSHAPE_STATS}}");
        assert!(result.is_err());
    }

    #[test]
    fn find_pr_template_prefers_repo_root() {
        let root = temp_dir_with_template("PULL_REQUEST_TEMPLATE.md");
        let found = find_pr_template(&root).expect("template found");
        assert_eq!(found, root.join("PULL_REQUEST_TEMPLATE.md"));
        cleanup_dir(&root);
    }

    #[test]
    fn interpolate_reads_and_renders_template() {
        let root = temp_dir_with_template(".github/PULL_REQUEST_TEMPLATE.md");
        let template_path = root.join(".github").join("PULL_REQUEST_TEMPLATE.md");

        let mut fs = MockFileSystem::new();
        fs.expect_read_to_string()
            .withf(move |path| path == template_path)
            .returning(|_| Ok("{{SHIPSHAPE_STATS}}".to_string()));

        let context = PrTemplateContext {
            stats: "stats".to_string(),
            fixes: "fixes".to_string(),
            ci: "ci".to_string(),
        };

        let rendered = interpolate_pr_template(&fs, &root, &context)
            .expect("rendered")
            .expect("template present");

        assert_eq!(rendered, "stats");

        cleanup_dir(&root);
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

    fn temp_dir_with_template(rel_path: &str) -> PathBuf {
        let root = std::env::temp_dir().join(unique_dir_name());
        let template_path = root.join(rel_path);
        if let Some(parent) = template_path.parent() {
            std::fs::create_dir_all(parent).expect("create template dir");
        }
        std::fs::write(&template_path, "placeholder").expect("write template");
        root
    }

    fn unique_dir_name() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        PathBuf::from(format!("shipshape_pr_template_test_{nanos}"))
    }

    fn cleanup_dir(root: &PathBuf) {
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
