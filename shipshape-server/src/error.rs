//! Unified error handling for the ShipShape server.
//!
//! Defines [`ServerError`] using `derive_more` and implements
//! [`actix_web::ResponseError`] so all HTTP handlers, database operations,
//! cryptographic helpers, and workflow runners share a single strong error type.

use std::io;
use std::num::ParseIntError;

use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use derive_more::{Display, Error, From};
use shipshape_core::ShipShapeError;

use crate::routes::ErrorResponse;

/// Unified error enum for all `shipshape-server` operations.
#[derive(Debug, Display, Error, From)]
pub enum ServerError {
    /// Underlying error from `shipshape-core`.
    #[display("core error: {_0}")]
    #[from]
    Core(ShipShapeError),

    /// Filesystem or process I/O error.
    #[display("io error: {_0}")]
    #[from]
    Io(io::Error),

    /// JSON serialization or deserialization failure.
    #[display("json error: {_0}")]
    #[from]
    Json(serde_json::Error),

    /// HTTP client request failure.
    #[display("http error: {_0}")]
    #[from]
    Http(reqwest::Error),

    /// R2D2 database connection pool error.
    #[display("db pool error: {_0}")]
    #[from]
    DbPool(diesel::r2d2::PoolError),

    /// Diesel database connection establishment error.
    #[display("db connection error: {_0}")]
    #[from]
    DbConnection(diesel::ConnectionError),

    /// Diesel query execution error.
    #[display("db query error: {_0}")]
    #[from]
    DbQuery(diesel::result::Error),

    /// Base64 decoding error in token encryption.
    #[display("base64 decode error: {_0}")]
    #[from]
    Base64Decode(base64::DecodeError),

    /// UTF-8 decoding error for decrypted token bytes.
    #[display("utf8 decode error: {_0}")]
    #[from]
    Utf8(std::string::FromUtf8Error),

    /// Environment variable lookup error.
    #[display("env var error: {_0}")]
    #[from]
    EnvVar(std::env::VarError),

    /// TCP port parsing error.
    #[display("port parse error: {_0}")]
    #[from]
    PortParse(ParseIntError),

    /// Actix blocking task execution error.
    #[display("blocking task error: {_0}")]
    #[from]
    BlockingTask(actix_web::error::BlockingError),

    /// Database migration failure.
    #[display("migration error: {message}")]
    #[from(ignore)]
    Migration {
        /// Diagnostic message from `diesel_migrations`.
        message: String,
    },

    /// Token encryption key configuration or length error.
    #[display("{detail}")]
    #[from(ignore)]
    CryptoKeyInvalid {
        /// Key validation failure reason.
        detail: String,
    },

    /// AES-256-GCM encryption or decryption failure.
    #[display("{detail}")]
    #[from(ignore)]
    CryptoOperationFailed {
        /// Cryptographic failure explanation.
        detail: String,
    },

    /// Authentication failure (HTTP 401).
    #[display("{reason}")]
    #[from(ignore)]
    Unauthorized {
        /// Human-readable authorization failure reason.
        reason: &'static str,
    },

    /// Client request validation failure (HTTP 400).
    #[display("{message}")]
    #[from(ignore)]
    BadRequest {
        /// Validation failure description.
        message: String,
    },

    /// Requested resource was not found (HTTP 404).
    #[display("{message}")]
    #[from(ignore)]
    NotFound {
        /// Missing resource description.
        message: String,
    },

    /// Synchronization primitive (`RwLock` or `Mutex`) was poisoned.
    #[display("{resource} unavailable")]
    #[from(ignore)]
    LockPoisoned {
        /// Name of the poisoned lock resource.
        resource: &'static str,
    },

    /// Git subprocess exited with a non-zero status (with credentials redacted).
    #[display("git {args:?} failed: {detail}")]
    #[from(ignore)]
    GitCommandFailed {
        /// Sanitized command arguments.
        args: Vec<String>,
        /// Sanitized stdout/stderr diagnostic detail.
        detail: String,
    },

    /// GitHub API returned a non-success response or unexpected payload.
    #[display("github api error ({status}): {detail}")]
    #[from(ignore)]
    GitHubApi {
        /// HTTP status code or label.
        status: String,
        /// Response body or diagnostic detail.
        detail: String,
    },

    /// GitLab API returned a non-success response or unexpected payload.
    #[display("gitlab api error ({status}): {detail}")]
    #[from(ignore)]
    GitLabApi {
        /// HTTP status code or label.
        status: String,
        /// Response body or diagnostic detail.
        detail: String,
    },

    /// Workflow validation or orchestration error.
    #[display("{message}")]
    #[from(ignore)]
    Workflow {
        /// Workflow error description.
        message: String,
    },
}

/// Convenience result alias for `shipshape-server` operations.
pub type ServerResult<T> = Result<T, ServerError>;

impl ServerError {
    /// Construct a [`ServerError::Workflow`] with the given message.
    ///
    /// # Arguments
    /// * `message` - Workflow error message.
    pub fn workflow(message: impl Into<String>) -> Self {
        Self::Workflow {
            message: message.into(),
        }
    }
}

impl ResponseError for ServerError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Unauthorized { .. } => StatusCode::UNAUTHORIZED,
            Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ErrorResponse {
            message: self.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ServerError, ServerResult};
    use actix_web::ResponseError as _;
    use actix_web::http::StatusCode;

    #[test]
    fn server_error_maps_status_codes_and_responses() -> ServerResult<()> {
        let unauth = ServerError::Unauthorized {
            reason: "missing authorization header",
        };
        assert_eq!(unauth.status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(unauth.error_response().status(), StatusCode::UNAUTHORIZED);

        let bad_req = ServerError::BadRequest {
            message: "diff path is required".to_string(),
        };
        assert_eq!(bad_req.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(bad_req.error_response().status(), StatusCode::BAD_REQUEST);

        let not_found = ServerError::NotFound {
            message: "diff file not found".to_string(),
        };
        assert_eq!(not_found.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(not_found.error_response().status(), StatusCode::NOT_FOUND);

        let internal = ServerError::workflow("internal failure");
        assert_eq!(internal.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            internal.error_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );

        let lock_err = ServerError::LockPoisoned {
            resource: "diff store",
        };
        assert_eq!(lock_err.to_string(), "diff store unavailable");

        let mig_err = ServerError::Migration {
            message: "migration failed".to_string(),
        };
        assert_eq!(mig_err.to_string(), "migration error: migration failed");
        Ok(())
    }
}
