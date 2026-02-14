#![deny(missing_docs)]
//! ShipShape core library.
//!
//! This crate contains the domain types and inspection primitives that power
//! the broader ShipShape platform.

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
pub use drydock::generate_ci_config;
pub use error::{Result, ShipShapeError};
pub use fs::{FileSystem, StdFileSystem};
pub use inspector::TokeiInspector;
pub use mechanic::Mechanic;
pub use mechanics::build_mechanics;
pub use pr_template::{
    PrTemplateContext, SHIPSHAPE_CI, SHIPSHAPE_FIXES, SHIPSHAPE_STATS, ensure_placeholders,
    find_pr_template, interpolate_pr_template,
};
pub use report::{
    CloneStatus, LaunchReport, RefitReport, RepoReport, format_language_stats,
    render_audit_markdown, render_json, render_launch_markdown, render_refit_markdown,
};
