//! Error types for ShipShape core.
//!
//! Defines the unified [`ShipShapeError`] enum powered by `derive_more` and the
//! crate-wide [`Result`] alias used across inspection, mechanics, drydock, and
//! reporting workflows.

use std::{fmt, io};

use derive_more::{Display, Error, From};

/// Unified error type for ShipShape core operations.
#[derive(Debug, Display, Error, From)]
pub enum ShipShapeError {
    /// An underlying filesystem or process I/O error.
    #[display("io error: {_0}")]
    #[from]
    Io(io::Error),

    /// A JSON serialization or deserialization failure.
    #[display("json error: {_0}")]
    #[from]
    Json(serde_json::Error),

    /// A string formatting failure.
    #[display("format error: {_0}")]
    #[from]
    Fmt(fmt::Error),

    /// A requested mechanic identifier was not found in the registry.
    #[display("unknown mechanic: {id}")]
    #[from(ignore)]
    UnknownMechanic {
        /// Unrecognized mechanic identifier.
        id: String,
    },

    /// A mechanic operation (audit, dry-run, or apply) is not configured.
    #[display("{operation} not configured for {id}")]
    #[from(ignore)]
    MechanicNotConfigured {
        /// Mechanic identifier.
        id: String,
        /// Operation name (`"audit"`, `"dry-run"`, or `"apply"`).
        operation: &'static str,
    },

    /// An external mechanic process terminated with a non-zero exit status.
    #[display("{id} failed ({status}): {detail}")]
    #[from(ignore)]
    MechanicExecutionFailed {
        /// Mechanic identifier.
        id: String,
        /// Exit status description.
        status: String,
        /// Merged stdout/stderr diagnostic detail.
        detail: String,
    },

    /// A pull-request template is missing one or more required placeholders.
    #[display("missing placeholders: {}", missing.join(", "))]
    #[from(ignore)]
    MissingPrPlaceholders {
        /// List of missing placeholder tokens.
        missing: Vec<&'static str>,
    },

    /// Input validation failed.
    #[display("invalid input: {message}")]
    #[from(ignore)]
    InvalidInput {
        /// Validation failure explanation.
        message: String,
    },

    /// A general domain error with a descriptive message.
    #[display("{_0}")]
    #[from(ignore)]
    Other(#[error(not(source))] String),
}

/// Convenience result type for ShipShape core operations.
pub type Result<T> = std::result::Result<T, ShipShapeError>;

#[cfg(test)]
mod tests {
    use super::{Result, ShipShapeError};
    use std::fmt::Write as _;
    use std::{fmt, io};

    /// Helper that fails with a formatting error to test `From<fmt::Error>`.
    struct FailingWriter;

    impl fmt::Write for FailingWriter {
        fn write_str(&mut self, _s: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }

    #[test]
    fn io_error_formats_and_converts() -> Result<()> {
        let error: ShipShapeError = io::Error::new(io::ErrorKind::NotFound, "missing").into();
        assert_eq!(error.to_string(), "io error: missing");
        assert!(matches!(error, ShipShapeError::Io(_)));
        Ok(())
    }

    #[test]
    fn json_error_formats_and_converts() {
        let error = serde_json::from_str::<serde_json::Value>("invalid")
            .map_err(ShipShapeError::from)
            .err();
        assert!(matches!(error, Some(ShipShapeError::Json(_))));
        assert!(error.is_some_and(|e| e.to_string().starts_with("json error:")));
    }

    #[test]
    fn fmt_error_formats_and_converts() {
        let mut writer = FailingWriter;
        let error = writer.write_str("boom").map_err(ShipShapeError::from).err();
        assert!(matches!(error, Some(ShipShapeError::Fmt(_))));
        assert!(error.is_some_and(|e| e.to_string().starts_with("format error:")));
    }

    #[test]
    fn structured_variants_format_messages() {
        let unknown = ShipShapeError::UnknownMechanic {
            id: "bad-id".to_string(),
        };
        assert_eq!(unknown.to_string(), "unknown mechanic: bad-id");

        let unconfigured = ShipShapeError::MechanicNotConfigured {
            id: "lib2nb2lib".to_string(),
            operation: "dry-run",
        };
        assert_eq!(
            unconfigured.to_string(),
            "dry-run not configured for lib2nb2lib"
        );

        let exec_failed = ShipShapeError::MechanicExecutionFailed {
            id: "cdd-c".to_string(),
            status: "exit status: 1".to_string(),
            detail: "syntax error".to_string(),
        };
        assert_eq!(
            exec_failed.to_string(),
            "cdd-c failed (exit status: 1): syntax error"
        );

        let missing = ShipShapeError::MissingPrPlaceholders {
            missing: vec!["{{SHIPSHAPE_STATS}}", "{{SHIPSHAPE_CI}}"],
        };
        assert_eq!(
            missing.to_string(),
            "missing placeholders: {{SHIPSHAPE_STATS}}, {{SHIPSHAPE_CI}}"
        );

        let invalid = ShipShapeError::InvalidInput {
            message: "empty path".to_string(),
        };
        assert_eq!(invalid.to_string(), "invalid input: empty path");

        let other = ShipShapeError::Other("shipshape failed".to_string());
        assert_eq!(other.to_string(), "shipshape failed");
    }
}
