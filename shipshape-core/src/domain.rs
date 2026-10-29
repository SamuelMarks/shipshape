//! Domain entities for ShipShape.
//!
//! Contains the core data structures representing language distributions,
//! coverage heuristics, audit violations, and fleet health reports.

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
    /// Test coverage ratio (`test_files / code_files`).
    pub test_coverage: f64,
    /// Documentation coverage ratio (`doc_files / code_files`).
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
    /// Aggregate health score, `0..=100`.
    pub health_score: u8,
}

#[cfg(test)]
mod tests {
    use super::{CoverageReport, FleetReport, LanguageDistribution, Violation};
    use crate::error::Result;
    use utoipa::PartialSchema as _;

    #[test]
    fn domain_types_roundtrip_json_and_schema() -> Result<()> {
        let mut language_stats = LanguageDistribution::new();
        language_stats.insert("Rust".to_string(), 80.0);
        language_stats.insert("Python".to_string(), 20.0);

        let report = FleetReport {
            language_stats,
            violations: vec![Violation {
                id: "v-1".to_string(),
                message: "Missing docs".to_string(),
            }],
            coverage: CoverageReport {
                code_files: 5,
                test_files: 2,
                doc_files: 1,
                test_coverage: 0.4,
                doc_coverage: 0.2,
                low_test_coverage: false,
                low_doc_coverage: false,
            },
            health_score: 98,
        };

        let json = serde_json::to_string(&report)?;
        let decoded: FleetReport = serde_json::from_str(&json)?;
        assert_eq!(decoded, report);

        let _coverage_schema = CoverageReport::schema();
        let _violation_schema = Violation::schema();
        let _fleet_schema = FleetReport::schema();
        Ok(())
    }
}
