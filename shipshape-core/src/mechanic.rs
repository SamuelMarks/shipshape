//! Mechanic trait definitions.
//!
//! Defines the [`Mechanic`] trait implemented by all audit and refit tools in
//! the ShipShape fleet.

use std::path::Path;

use crate::domain::Violation;
use crate::error::Result;

/// A tool that can audit and fix code in a repository workspace.
pub trait Mechanic {
    /// Returns the unique canonical identifier of the tool (e.g., `"lib2nb2lib"`).
    fn id(&self) -> &str;

    /// Checks for issues in the repository at `path` and returns a list of violations.
    ///
    /// # Arguments
    /// * `path` - Repository root path to audit.
    fn audit(&self, path: &Path) -> Result<Vec<Violation>>;

    /// Applies a fix in dry-run mode on `path` and returns the diff output.
    ///
    /// # Arguments
    /// * `path` - Repository root path to inspect in dry-run mode.
    fn dry_run(&self, path: &Path) -> Result<String>;

    /// Applies fixes to the filesystem at `path`, returning `true` if changes were made.
    ///
    /// # Arguments
    /// * `path` - Repository root path to modify.
    fn apply(&self, path: &Path) -> Result<bool>;
}
