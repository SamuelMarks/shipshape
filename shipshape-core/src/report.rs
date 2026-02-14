//! Report formatting utilities for ShipShape outputs.

use std::fmt::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Violation;
use crate::domain::LanguageDistribution;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub fn render_audit_markdown(reports: &[RepoReport]) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# ShipShape Audit Report\n");
    for report in reports {
        let _ = writeln!(output, "## {}\n", report.source);
        append_clone_status(&mut output, &report.clone_status, &report.path);
        append_language_stats(&mut output, report.language_stats.as_ref());
        append_violations(&mut output, &report.violations);
        append_errors(&mut output, "Audit errors", &report.audit_errors);
        let _ = writeln!(output);
    }
    output
}

/// Render a list of refit reports as Markdown.
pub fn render_refit_markdown(reports: &[RefitReport]) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# ShipShape Refit Report\n");
    for report in reports {
        let _ = writeln!(output, "## {}\n", report.source);
        append_clone_status(&mut output, &report.clone_status, &report.path);
        append_list(
            &mut output,
            "Refit results",
            &report.results,
            "No refit results.",
        );
        append_errors(&mut output, "Refit errors", &report.errors);
        let _ = writeln!(output);
    }
    output
}

/// Render a list of launch reports as Markdown.
pub fn render_launch_markdown(reports: &[LaunchReport]) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# ShipShape Launch Report\n");
    for report in reports {
        let _ = writeln!(output, "## {}\n", report.source);
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
        let _ = writeln!(output);
    }
    output
}

/// Render any serializable report payload as JSON.
pub fn render_json<T: Serialize + ?Sized>(payload: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(payload)
}

/// Format language stats sorted by percentage.
pub fn format_language_stats(stats: &LanguageDistribution) -> Vec<(String, f64)> {
    let mut items: Vec<(String, f64)> = stats.iter().map(|(k, v)| (k.clone(), *v)).collect();
    items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    items
}

fn append_clone_status(output: &mut String, status: &CloneStatus, path: &PathBuf) {
    let _ = writeln!(output, "- Path: `{}`", path.display());
    match status {
        CloneStatus::Cloned => {
            let _ = writeln!(output, "- Status: cloned");
        }
        CloneStatus::Local => {
            let _ = writeln!(output, "- Status: local");
        }
        CloneStatus::Pending => {
            let _ = writeln!(output, "- Status: pending");
        }
        CloneStatus::Failed(error) => {
            let _ = writeln!(output, "- Status: failed ({error})");
        }
    }
    let _ = writeln!(output);
}

fn append_language_stats(output: &mut String, stats: Option<&LanguageDistribution>) {
    match stats {
        Some(stats) if stats.is_empty() => {
            let _ = writeln!(output, "### Languages\nNo languages detected.\n");
        }
        Some(stats) => {
            let _ = writeln!(output, "### Languages");
            for (language, percent) in format_language_stats(stats) {
                let _ = writeln!(output, "- {language}: {percent:.2}%");
            }
            let _ = writeln!(output);
        }
        None => {
            let _ = writeln!(output, "### Languages\nLanguages unavailable.\n");
        }
    }
}

fn append_violations(output: &mut String, violations: &[Violation]) {
    if violations.is_empty() {
        let _ = writeln!(output, "### Violations\nNo violations found.\n");
        return;
    }
    let _ = writeln!(output, "### Violations");
    for violation in violations {
        let _ = writeln!(output, "- [{}] {}", violation.id, violation.message);
    }
    let _ = writeln!(output);
}

fn append_errors(output: &mut String, title: &str, errors: &[String]) {
    append_list(output, title, errors, "No errors reported.");
}

fn append_list(output: &mut String, title: &str, items: &[String], empty_message: &str) {
    if items.is_empty() {
        let _ = writeln!(output, "### {title}\n{empty_message}\n");
        return;
    }
    let _ = writeln!(output, "### {title}");
    for item in items {
        let _ = writeln!(output, "- {item}");
    }
    let _ = writeln!(output);
}

fn append_code_block(output: &mut String, title: &str, contents: Option<&str>, empty: &str) {
    let _ = writeln!(output, "### {title}");
    match contents {
        Some(contents) => {
            let _ = writeln!(output, "```text\n{contents}\n```\n");
        }
        None => {
            let _ = writeln!(output, "{empty}\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Violation;
    use std::collections::BTreeMap;

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
    fn renders_audit_markdown() {
        let report = sample_audit_report();
        let output = render_audit_markdown(&[report]);
        assert!(output.contains("ShipShape Audit Report"));
        assert!(output.contains("Status: cloned"));
        assert!(output.contains("Rust: 55.50%"));
        assert!(output.contains("[docs] Missing docs"));
        assert!(output.contains("lint failed"));
    }

    #[test]
    fn renders_refit_markdown() {
        let mut report = RefitReport::new("repo".to_string(), PathBuf::from("/tmp/repo"));
        report.clone_status = CloneStatus::Local;
        report.results.push("mechanic: applied".to_string());
        let output = render_refit_markdown(&[report]);
        assert!(output.contains("ShipShape Refit Report"));
        assert!(output.contains("Status: local"));
        assert!(output.contains("mechanic: applied"));
    }

    #[test]
    fn renders_launch_markdown() {
        let mut report = LaunchReport::new("repo".to_string(), PathBuf::from("/tmp/repo"));
        report.clone_status = CloneStatus::Failed("boom".to_string());
        report.dockerfile = Some("FROM rust".to_string());
        let output = render_launch_markdown(&[report]);
        assert!(output.contains("ShipShape Launch Report"));
        assert!(output.contains("Status: failed (boom)"));
        assert!(output.contains("FROM rust"));
    }

    #[test]
    fn renders_json_payload() {
        let report = sample_audit_report();
        let json = render_json(&vec![report]).expect("json");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["cloneStatus"]["status"], "cloned");
    }

    #[test]
    fn formats_language_stats_sorted() {
        let mut stats = BTreeMap::new();
        stats.insert("Go".to_string(), 10.0);
        stats.insert("Rust".to_string(), 30.0);
        let ordered = format_language_stats(&stats);
        assert_eq!(ordered[0].0, "Rust");
    }
}
