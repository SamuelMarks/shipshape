//! Error types for ShipShape core.

use std::{error::Error, fmt, io};

/// Error type for ShipShape core operations.
#[derive(Debug)]
pub enum ShipShapeError {
    /// An underlying I/O error.
    Io(io::Error),
    /// A catch-all error with a message.
    Other(String),
}

impl fmt::Display for ShipShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Other(message) => write!(f, "{message}"),
        }
    }
}

impl Error for ShipShapeError {}

impl From<io::Error> for ShipShapeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Convenience result type for ShipShape core.
pub type Result<T> = std::result::Result<T, ShipShapeError>;

#[cfg(test)]
mod tests {
    use super::ShipShapeError;
    use std::io;

    #[test]
    fn io_error_formats_message() {
        let error = ShipShapeError::Io(io::Error::new(io::ErrorKind::Other, "boom"));
        assert_eq!(format!("{error}"), "io error: boom");
    }

    #[test]
    fn other_error_formats_message() {
        let error = ShipShapeError::Other("shipshape failed".to_string());
        assert_eq!(format!("{error}"), "shipshape failed");
    }

    #[test]
    fn from_io_error_maps_variant() {
        let error: ShipShapeError = io::Error::new(io::ErrorKind::NotFound, "missing").into();
        match error {
            ShipShapeError::Io(inner) => {
                assert_eq!(inner.kind(), io::ErrorKind::NotFound);
            }
            ShipShapeError::Other(_) => panic!("expected Io variant"),
        }
    }
}
