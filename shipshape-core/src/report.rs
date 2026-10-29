//! Report formatting utilities for ShipShape outputs.
//!
//! Defines [`CloneStatus`], [`RepoReport`], [`RefitReport`], and [`LaunchReport`],
//! along with Markdown and JSON serialization helpers.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Violation;
use crate::domain::LanguageDistribution;
use crate::error::Result;

/// Status of a repository clone or local load operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "message", rename_all = "snake_case")]
pub enum CloneStatus {
    /// Clone operation has not started.
    Pending,
    /// Repository was cloned successfully.
    Cloned,
    /// Repository was loaded from a local path.
    Local,
    /// Clone or load failed with an error message.
    Failed(String),
}

/// Audit report for a repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoReport {
    /// Repository source (URL or path).
    pub source: String,
    /// Local path used for inspection.
    pub path: PathBuf,
    /// Clone status.
    pub clone_status: CloneStatus,
    /// Language distribution statistics.
    pub language_stats: Option<LanguageDistribution>,
    /// Violations found during audit.
    pub violations: Vec<Violation>,
    /// Errors encountered during auditing.
    pub audit_errors: Vec<String>,
}

impl RepoReport {
    /// Create a new report for a repository.
    ///
    /// # Arguments
    /// * `source` - Repository URL or path string.
    /// * `path` - Local workspace directory path.
    #[must_use]
    pub fn new(source: String, path: PathBuf) -> Self {
        Self {
            source,
            path,
            clone_status: CloneStatus::Pending,
            language_stats: None,
            violations: Vec::new(),
            audit_errors: Vec::new(),
        }
    }

    /// Create a report for a failed repository.
    ///
    /// # Arguments
    /// * `source` - Repository URL or path string.
    /// * `path` - Local workspace directory path.
    /// * `error` - Error description explaining the failure.
    pub fn failed(source: String, path: PathBuf, error: impl Into<String>) -> Self {
        Self {
            source,
            path,
            clone_status: CloneStatus::Failed(error.into()),
            language_stats: None,
            violations: Vec::new(),
            audit_errors: Vec::new(),
        }
    }
}

/// Refit report for a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefitReport {
    /// Repository source (URL or path).
    pub source: String,
    /// Local path used for refit.
    pub path: PathBuf,
    /// Clone status.
    pub clone_status: CloneStatus,
    /// Result lines for refit mechanics.
    pub results: Vec<String>,
    /// Errors encountered during refit.
    pub errors: Vec<String>,
}

impl RefitReport {
    /// Create a new refit report.
    ///
    /// # Arguments
    /// * `source` - Repository URL or path string.
    /// * `path` - Local workspace directory path.
    #[must_use]
    pub fn new(source: String, path: PathBuf) -> Self {
        Self {
            source,
            path,
            clone_status: CloneStatus::Pending,
            results: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Create a refit report for a failed repository.
    ///
    /// # Arguments
    /// * `source` - Repository URL or path string.
    /// * `path` - Local workspace directory path.
    /// * `error` - Error description explaining the failure.
    pub fn failed(source: String, path: PathBuf, error: impl Into<String>) -> Self {
        Self {
            source,
            path,
            clone_status: CloneStatus::Failed(error.into()),
            results: Vec::new(),
            errors: Vec::new(),
        }
    }
}

/// Launch report for a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchReport {
    /// Repository source (URL or path).
    pub source: String,
    /// Local path used for launch.
    pub path: PathBuf,
    /// Clone status.
    pub clone_status: CloneStatus,
    /// Generated Dockerfile contents, if available.
    pub dockerfile: Option<String>,
    /// Generated CI config contents, if available.
    pub ci_config: Option<String>,
    /// Errors encountered during launch.
    pub errors: Vec<String>,
}

impl LaunchReport {
    /// Create a new launch report.
    ///
    /// # Arguments
    /// * `source` - Repository URL or path string.
    /// * `path` - Local workspace directory path.
    #[must_use]
    pub fn new(source: String, path: PathBuf) -> Self {
        Self {
            source,
            path,
            clone_status: CloneStatus::Pending,
            dockerfile: None,
            ci_config: None,
            errors: Vec::new(),
        }
    }

    /// Create a launch report for a failed repository.
    ///
    /// # Arguments
    /// * `source` - Repository URL or path string.
    /// * `path` - Local workspace directory path.
    /// * `error` - Error description explaining the failure.
    pub fn failed(source: String, path: PathBuf, error: impl Into<String>) -> Self {
        Self {
            source,
            path,
            clone_status: CloneStatus::Failed(error.into()),
            dockerfile: None,
            ci_config: None,
            errors: Vec::new(),
        }
    }
}

/// Render a list of audit reports as Markdown.
///
/// # Arguments
/// * `reports` - Slice of repository audit reports.
#[must_use]
pub fn render_audit_markdown(reports: &[RepoReport]) -> String {
    let mut output = String::from("# ShipShape Audit Report\n\n");
    for report in reports {
        output.push_str(&format!("## {}\n\n", report.source));
        append_clone_status(&mut output, &report.clone_status, &report.path);
        append_language_stats(&mut output, report.language_stats.as_ref());
        append_violations(&mut output, &report.violations);
        append_errors(&mut output, "Audit errors", &report.audit_errors);
        output.push('\n');
    }
    output
}

/// Render a list of refit reports as Markdown.
///
/// # Arguments
/// * `reports` - Slice of repository refit reports.
#[must_use]
pub fn render_refit_markdown(reports: &[RefitReport]) -> String {
    let mut output = String::from("# ShipShape Refit Report\n\n");
    for report in reports {
        output.push_str(&format!("## {}\n\n", report.source));
        append_clone_status(&mut output, &report.clone_status, &report.path);
        append_list(
            &mut output,
            "Refit results",
            &report.results,
            "No refit results.",
        );
        append_errors(&mut output, "Refit errors", &report.errors);
        output.push('\n');
    }
    output
}

/// Render a list of launch reports as Markdown.
///
/// # Arguments
/// * `reports` - Slice of repository launch reports.
#[must_use]
pub fn render_launch_markdown(reports: &[LaunchReport]) -> String {
    let mut output = String::from("# ShipShape Launch Report\n\n");
    for report in reports {
        output.push_str(&format!("## {}\n\n", report.source));
        append_clone_status(&mut output, &report.clone_status, &report.path);
        append_code_block(
            &mut output,
            "Dockerfile",
            report.dockerfile.as_deref(),
            "Dockerfile unavailable.",
        );
        append_code_block(
            &mut output,
            "CI config",
            report.ci_config.as_deref(),
            "CI config unavailable.",
        );
        append_errors(&mut output, "Launch errors", &report.errors);
        output.push('\n');
    }
    output
}

/// Render any serializable report payload as pretty-printed JSON.
///
/// # Arguments
/// * `payload` - Serializable value to format as JSON.
pub fn render_json<T: Serialize + ?Sized>(payload: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(payload)?)
}

/// Format language stats sorted descending by percentage.
///
/// # Arguments
/// * `stats` - Language distribution map.
#[must_use]
pub fn format_language_stats(stats: &LanguageDistribution) -> Vec<(String, f64)> {
    let mut items: Vec<(String, f64)> = stats.iter().map(|(k, v)| (k.clone(), *v)).collect();
    items.sort_by(|a, b| b.1.total_cmp(&a.1));
    items
}

/// Append path and clone status bullets to `output`.
///
/// # Arguments
/// * `output` - Target Markdown string buffer.
/// * `status` - Repository clone status.
/// * `path` - Repository local path.
fn append_clone_status(output: &mut String, status: &CloneStatus, path: &Path) {
    output.push_str(&format!(
        "- Path: `{}`
",
        path.display()
    ));
    match status {
        CloneStatus::Cloned => output.push_str(
            "- Status: cloned

",
        ),
        CloneStatus::Local => output.push_str(
            "- Status: local

",
        ),
        CloneStatus::Pending => output.push_str(
            "- Status: pending

",
        ),
        CloneStatus::Failed(error) => output.push_str(&format!(
            "- Status: failed ({error})

"
        )),
    }
}

/// Append the `### Languages` section to `output`.
///
/// # Arguments
/// * `output` - Target Markdown string buffer.
/// * `stats` - Optional language distribution map.
fn append_language_stats(output: &mut String, stats: Option<&LanguageDistribution>) {
    match stats {
        Some(stats) if stats.is_empty() => {
            output.push_str(
                "### Languages
No languages detected.

",
            );
        }
        Some(stats) => {
            output.push_str("### Languages\n");
            for (language, percent) in format_language_stats(stats) {
                output.push_str(&format!("- {language}: {percent:.2}%\n"));
            }
            output.push('\n');
        }
        None => {
            output.push_str("### Languages\nLanguages unavailable.\n\n");
        }
    }
}

/// Append the `### Violations` section to `output`.
///
/// # Arguments
/// * `output` - Target Markdown string buffer.
/// * `violations` - Slice of violations to render.
fn append_violations(output: &mut String, violations: &[Violation]) {
    if violations.is_empty() {
        output.push_str("### Violations\nNo violations found.\n\n");
        return;
    }
    output.push_str("### Violations\n");
    for violation in violations {
        output.push_str(&format!("- [{}] {}\n", violation.id, violation.message));
    }
    output.push('\n');
}

/// Append an error list section to `output`.
///
/// # Arguments
/// * `output` - Target Markdown string buffer.
/// * `title` - Section title.
/// * `errors` - Error messages to render.
fn append_errors(output: &mut String, title: &str, errors: &[String]) {
    append_list(output, title, errors, "No errors reported.");
}

/// Append a generic bullet list section or `empty_message` to `output`.
///
/// # Arguments
/// * `output` - Target Markdown string buffer.
/// * `title` - Section heading title.
/// * `items` - Bullet items.
/// * `empty_message` - Fallback text when `items` is empty.
fn append_list(output: &mut String, title: &str, items: &[String], empty_message: &str) {
    if items.is_empty() {
        output.push_str(&format!("### {title}\n{empty_message}\n\n"));
        return;
    }
    output.push_str(&format!("### {title}\n"));
    for item in items {
        output.push_str(&format!("- {item}\n"));
    }
    output.push('\n');
}

/// Append a fenced code block section or `empty` fallback to `output`.
///
/// # Arguments
/// * `output` - Target Markdown string buffer.
/// * `title` - Section heading title.
/// * `contents` - Optional code block body.
/// * `empty` - Fallback message when `contents` is `None`.
fn append_code_block(output: &mut String, title: &str, contents: Option<&str>, empty: &str) {
    output.push_str(&format!(
        "### {title}
"
    ));
    match contents {
        Some(contents) => {
            output.push_str(&format!(
                "```text
{contents}
```

"
            ));
        }
        None => {
            output.push_str(&format!(
                "{empty}

"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Violation;
    use std::collections::BTreeMap;

    /// Build a populated sample [`RepoReport`].
    fn sample_audit_report() -> RepoReport {
        let mut report = RepoReport::new(
            "https://example.com/repo.git".to_string(),
            PathBuf::from("/tmp/repo"),
        );
        report.clone_status = CloneStatus::Cloned;
        let mut stats = BTreeMap::new();
        stats.insert("Rust".to_string(), 55.5);
        report.language_stats = Some(stats);
        report.violations = vec![Violation {
            id: "docs".to_string(),
            message: "Missing docs".to_string(),
        }];
        report.audit_errors = vec!["lint failed".to_string()];
        report
    }

    #[test]
    fn constructors_initialize_pending_and_failed_states() {
        let audit_fail = RepoReport::failed("s".to_string(), PathBuf::from("/p"), "err");
        assert_eq!(
            audit_fail.clone_status,
            CloneStatus::Failed("err".to_string())
        );

        let refit_fail = RefitReport::failed("s".to_string(), PathBuf::from("/p"), "err");
        assert_eq!(
            refit_fail.clone_status,
            CloneStatus::Failed("err".to_string())
        );

        let launch_fail = LaunchReport::failed("s".to_string(), PathBuf::from("/p"), "err");
        assert_eq!(
            launch_fail.clone_status,
            CloneStatus::Failed("err".to_string())
        );
    }

    #[test]
    fn renders_audit_markdown_for_populated_empty_and_unavailable_states() {
        let populated = sample_audit_report();

        let mut empty_stats_report =
            RepoReport::new("local-empty".to_string(), PathBuf::from("/tmp/empty"));
        empty_stats_report.language_stats = Some(BTreeMap::new());

        let unavailable_report =
            RepoReport::failed("failed-repo".to_string(), PathBuf::from("/tmp/fail"), "net");

        let output = render_audit_markdown(&[populated, empty_stats_report, unavailable_report]);
        assert!(output.contains("ShipShape Audit Report"));
        assert!(output.contains("Status: cloned"));
        assert!(output.contains("Status: pending"));
        assert!(output.contains("Status: failed (net)"));
        assert!(output.contains("Rust: 55.50%"));
        assert!(output.contains("No languages detected."));
        assert!(output.contains("Languages unavailable."));
        assert!(output.contains("[docs] Missing docs"));
        assert!(output.contains("No violations found."));
        assert!(output.contains("lint failed"));
        assert!(output.contains("No errors reported."));
    }

    #[test]
    fn renders_refit_markdown_for_populated_and_empty_results() {
        let mut populated = RefitReport::new("repo".to_string(), PathBuf::from("/tmp/repo"));
        populated.clone_status = CloneStatus::Local;
        populated.results.push("mechanic: applied".to_string());

        let empty = RefitReport::new("repo-2".to_string(), PathBuf::from("/tmp/repo-2"));

        let output = render_refit_markdown(&[populated, empty]);
        assert!(output.contains("ShipShape Refit Report"));
        assert!(output.contains("Status: local"));
        assert!(output.contains("mechanic: applied"));
        assert!(output.contains("No refit results."));
    }

    #[test]
    fn renders_launch_markdown_for_present_and_absent_configs() {
        let mut report = LaunchReport::new("repo".to_string(), PathBuf::from("/tmp/repo"));
        report.clone_status = CloneStatus::Failed("boom".to_string());
        report.dockerfile = Some("FROM rust".to_string());
        report.ci_config = Some(
            "stages:
  - test"
                .to_string(),
        );

        let empty = LaunchReport::new("repo-2".to_string(), PathBuf::from("/tmp/repo-2"));

        let output = render_launch_markdown(&[report, empty]);
        assert!(output.contains("ShipShape Launch Report"));
        assert!(output.contains("Status: failed (boom)"));
        assert!(output.contains("FROM rust"));
        assert!(output.contains(
            "stages:
  - test"
        ));
        assert!(output.contains("Dockerfile unavailable."));
        assert!(output.contains("CI config unavailable."));
    }

    #[test]
    fn renders_json_payload() -> Result<()> {
        let report = sample_audit_report();
        let json = render_json(&vec![report])?;
        let parsed: serde_json::Value = serde_json::from_str(&json)?;
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["cloneStatus"]["status"], "cloned");
        Ok(())
    }

    #[test]
    fn formats_language_stats_sorted() {
        let mut stats = BTreeMap::new();
        stats.insert("Go".to_string(), 10.0);
        stats.insert("Rust".to_string(), 30.0);
        let ordered = format_language_stats(&stats);
        assert_eq!(ordered[0].0, "Rust");
        assert_eq!(ordered[1].0, "Go");
    }
}
