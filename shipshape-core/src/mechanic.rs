//! Mechanic trait definitions.

use std::path::Path;

use crate::domain::Violation;
use crate::error::Result;

/// A tool that can audit and fix code.
pub trait Mechanic {
    /// Returns the unique ID of the tool (e.g., "lib2nb2lib").
    fn id(&self) -> &str;
    /// Checks for issues and returns a list of violations.
    fn audit(&self, path: &Path) -> Result<Vec<Violation>>;
    /// Applies a fix in dry-run mode and returns the diff output.
    fn dry_run(&self, path: &Path) -> Result<String>;
    /// Applies fixes to the filesystem, returning true if changes were made.
    fn apply(&self, path: &Path) -> Result<bool>;
}
