//! Domain entities for ShipShape.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A mapping of language names to their percentage of total lines.
pub type LanguageDistribution = BTreeMap<String, f64>;

/// Coverage heuristics for test and documentation files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CoverageReport {
    /// Number of code files detected.
    pub code_files: usize,
    /// Number of test files detected.
    pub test_files: usize,
    /// Number of documentation files detected.
    pub doc_files: usize,
    /// Test coverage ratio (test files / code files).
    pub test_coverage: f64,
    /// Documentation coverage ratio (doc files / code files).
    pub doc_coverage: f64,
    /// Whether test coverage is below the heuristic threshold.
    pub low_test_coverage: bool,
    /// Whether documentation coverage is below the heuristic threshold.
    pub low_doc_coverage: bool,
}

/// A code quality violation discovered during an audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Violation {
    /// Stable identifier for the violation type.
    pub id: String,
    /// Human-readable summary of the issue.
    pub message: String,
}

/// Represents the health status of a repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FleetReport {
    /// Language usage statistics for the repository.
    pub language_stats: LanguageDistribution,
    /// Violations discovered during inspection.
    pub violations: Vec<Violation>,
    /// Coverage heuristics for tests and documentation.
    pub coverage: CoverageReport,
    /// Aggregate health score, 0-100.
    pub health_score: u8,
}
