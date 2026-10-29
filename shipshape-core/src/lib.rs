#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
//! ShipShape core library.
//!
//! This crate contains the domain types, filesystem abstractions, repository
//! inspection primitives, code-repair mechanics, CI drydock generators, and
//! report formatters that power the broader ShipShape platform.

pub mod domain;
pub mod drydock;
pub mod error;
pub mod fs;
pub mod inspector;
pub mod mechanic;
/// Mechanic registry and orchestration helpers.
pub mod mechanics;
pub mod pr_template;
pub mod report;

pub use domain::{CoverageReport, FleetReport, LanguageDistribution, Violation};
pub use drydock::{generate_ci_config, generate_ci_config_with_fs};
pub use error::{Result, ShipShapeError};
pub use fs::{
    FileSystem, PYTHON_PACKAGING_MARKERS, StdFileSystem, has_python_packaging, is_hidden_path,
    is_notebook_path,
};
pub use inspector::{TokeiInspector, compute_health_score, inspect_coverage};
pub use mechanic::Mechanic;
pub use mechanics::{
    CommandOutput, CommandRunner, StdCommandRunner, build_mechanics, build_mechanics_with,
};
pub use pr_template::{
    PrTemplateContext, SHIPSHAPE_CI, SHIPSHAPE_FIXES, SHIPSHAPE_STATS, ensure_placeholders,
    find_pr_template, find_pr_template_with_fs, interpolate_pr_template,
};
pub use report::{
    CloneStatus, LaunchReport, RefitReport, RepoReport, format_language_stats,
    render_audit_markdown, render_json, render_launch_markdown, render_refit_markdown,
};
