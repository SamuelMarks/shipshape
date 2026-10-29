#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
//! ShipShape command-line interface.
//!
//! Provides batch audit, refit, launch, and OAuth device-flow login workflows
//! for local and remote repositories.

mod auth;

use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use auth::LoginArgs;
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use derive_more::{Display, Error, From};
use shipshape_core::{
    CloneStatus, LanguageDistribution, LaunchReport, Mechanic, RefitReport, RepoReport,
    ShipShapeError, StdFileSystem, TokeiInspector, build_mechanics, format_language_stats,
    generate_ci_config, is_hidden_path, render_audit_markdown, render_json, render_launch_markdown,
    render_refit_markdown,
};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Unified error enum for the ShipShape CLI powered by `derive_more`.
#[derive(Debug, Display, Error, From)]
pub enum CliError {
    /// Underlying error from `shipshape-core`.
    #[display("core error: {_0}")]
    #[from]
    Core(ShipShapeError),

    /// Filesystem or subprocess I/O error.
    #[display("io error: {_0}")]
    #[from]
    Io(io::Error),

    /// JSON serialization or deserialization failure.
    #[display("json error: {_0}")]
    #[from]
    Json(serde_json::Error),

    /// HTTP request error during authentication.
    #[display("http error: {_0}")]
    #[from]
    Http(reqwest::Error),

    /// Tokio semaphore acquisition error.
    #[display("semaphore error: {_0}")]
    #[from]
    Semaphore(tokio::sync::AcquireError),

    /// Tokio task join error.
    #[display("task join error: {_0}")]
    #[from]
    Join(tokio::task::JoinError),

    /// CLI argument parsing failure.
    #[display("cli parse error: {_0}")]
    #[from]
    Clap(clap::Error),

    /// Repository URL provided via `--url` was empty.
    #[display("url cannot be empty")]
    EmptyUrl,

    /// No repository source flag (`--file`, `--url`, `--dir`, or `--path`) was provided.
    #[display("no repository source provided")]
    MissingSource,

    /// `git clone` subprocess exited with a non-zero status.
    #[display("git clone failed with status {status}")]
    #[from(ignore)]
    GitCloneFailed {
        /// Human-readable exit status.
        status: String,
    },

    /// Server URL provided to `login` was empty.
    #[display("server url is required")]
    EmptyServerUrl,

    /// OAuth authorize URL did not end with `/authorize`.
    #[display("authorize_url missing /authorize suffix: {url}")]
    #[from(ignore)]
    InvalidAuthorizeUrl {
        /// Invalid authorize URL string.
        url: String,
    },

    /// GitHub device code expired before the user completed authorization.
    #[display("device code expired")]
    DeviceCodeExpired,

    /// The user denied the GitHub device authorization request.
    #[display("access denied")]
    DeviceAccessDenied,

    /// Device authorization polling exceeded the expiration deadline.
    #[display("device authorization timed out")]
    DeviceAuthorizationTimedOut,

    /// General OAuth device flow error.
    #[display("{message}")]
    #[from(ignore)]
    DeviceFlow {
        /// Diagnostic error message.
        message: String,
    },

    /// No local configuration directory could be resolved for storing auth credentials.
    #[display("unable to resolve auth storage path")]
    AuthStoragePathUnavailable,
}

/// Convenience result alias for CLI operations.
pub type CliResult<T> = Result<T, CliError>;

/// Top-level CLI argument parser.
#[derive(Parser)]
#[command(name = "shipshape", version, about = "ShipShape CLI")]
struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    command: Commands,
}

/// Repository source arguments (`--file`, `--url`, `--dir`, or `--path`).
#[derive(Args, Clone)]
#[command(group(
    ArgGroup::new("source")
        .required(true)
        .args(&["file", "url", "dir", "path"])
))]
struct RepoSourceArgs {
    /// File containing repository URLs (one per line).
    #[arg(short, long)]
    file: Option<PathBuf>,
    /// Single repository URL to clone.
    #[arg(long)]
    url: Option<String>,
    /// Directory containing repositories to audit locally.
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Local repository path to audit.
    #[arg(long)]
    path: Option<PathBuf>,
}

/// Repository cloning concurrency and output directory arguments.
#[derive(Args, Clone)]
struct CloneArgs {
    /// Output directory to clone into.
    #[arg(short, long, default_value = "shipshape-batch")]
    output: PathBuf,
    /// Maximum number of concurrent clones.
    #[arg(short = 'j', long, default_value_t = 5)]
    concurrency: usize,
}

/// Mechanic selection arguments.
#[derive(Args, Clone)]
struct MechanicArgs {
    /// Mechanic IDs to run (repeatable or comma-separated).
    #[arg(long, value_delimiter = ',')]
    mechanic: Vec<String>,
}

/// Report formatting and destination arguments.
#[derive(Args, Clone)]
struct OutputArgs {
    /// Output format for report data.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Write the report to a file instead of stdout.
    #[arg(long = "report-output")]
    report_output: Option<PathBuf>,
}

/// Supported CLI report output formats.
#[derive(ValueEnum, Copy, Clone, Debug, Eq, PartialEq)]
enum OutputFormat {
    /// Plain-text console summary.
    Text,
    /// Pretty-printed JSON array.
    Json,
    /// Markdown document.
    Markdown,
}

/// CLI subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Clone and audit repositories from a URL, file, directory, or local path.
    Batch {
        /// Repository input source.
        #[command(flatten)]
        source: RepoSourceArgs,
        /// Clone output and concurrency options.
        #[command(flatten)]
        clone: CloneArgs,
        /// Mechanic IDs to audit with.
        #[command(flatten)]
        mechanics: MechanicArgs,
        /// Report output format and path.
        #[command(flatten)]
        report: OutputArgs,
    },
    /// Audit repositories from a URL, file, directory, or local path.
    Audit {
        /// Repository input source.
        #[command(flatten)]
        source: RepoSourceArgs,
        /// Clone output and concurrency options.
        #[command(flatten)]
        clone: CloneArgs,
        /// Mechanic IDs to audit with.
        #[command(flatten)]
        mechanics: MechanicArgs,
        /// Report output format and path.
        #[command(flatten)]
        report: OutputArgs,
    },
    /// Run refit mechanics in dry-run mode or apply fixes.
    Refit {
        /// Repository input source.
        #[command(flatten)]
        source: RepoSourceArgs,
        /// Clone output and concurrency options.
        #[command(flatten)]
        clone: CloneArgs,
        /// Mechanic IDs to execute.
        #[command(flatten)]
        mechanics: MechanicArgs,
        /// Report output format and path.
        #[command(flatten)]
        report: OutputArgs,
        /// Apply fixes instead of dry-run output.
        #[arg(long)]
        apply: bool,
    },
    /// Generate launch (CI) configs for repositories.
    Launch {
        /// Repository input source.
        #[command(flatten)]
        source: RepoSourceArgs,
        /// Clone output and concurrency options.
        #[command(flatten)]
        clone: CloneArgs,
        /// Report output format and path.
        #[command(flatten)]
        report: OutputArgs,
    },
    /// Authenticate the CLI via the GitHub device flow.
    Login(LoginArgs),
}

#[cfg(not(test))]
#[tokio::main]
async fn main() -> CliResult<()> {
    run_cli_from_iter(std::env::args_os()).await
}

#[cfg(test)]
fn main() {}

/// Parse CLI arguments from `args` and execute the requested subcommand.
///
/// # Arguments
/// * `args` - Iterator of command-line arguments (including program name at index 0).
pub async fn run_cli_from_iter<I, T>(args: I) -> CliResult<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;
    dispatch_cli(cli).await
}

/// Dispatch a parsed [`Cli`] command to its workflow handler.
///
/// # Arguments
/// * `cli` - Parsed command-line structure.
async fn dispatch_cli(cli: Cli) -> CliResult<()> {
    match cli.command {
        Commands::Batch {
            source,
            clone,
            mechanics,
            report,
        }
        | Commands::Audit {
            source,
            clone,
            mechanics,
            report,
        } => {
            let source = resolve_source_args(&source)?;
            run_audit(
                source,
                clone.output,
                clone.concurrency,
                mechanics.mechanic,
                report,
            )
            .await
        }
        Commands::Refit {
            source,
            clone,
            mechanics,
            report,
            apply,
        } => {
            let source = resolve_source_args(&source)?;
            run_refit(
                source,
                clone.output,
                clone.concurrency,
                mechanics.mechanic,
                report,
                apply,
            )
            .await
        }
        Commands::Launch {
            source,
            clone,
            report,
        } => {
            let source = resolve_source_args(&source)?;
            run_launch(source, clone.output, clone.concurrency, report).await
        }
        Commands::Login(args) => auth::run_login(args).await,
    }
}

/// Trait unifying report initialization and clone-status mutation across batch workflows.
trait BatchReportItem: Send + 'static {
    /// Construct a new report in [`CloneStatus::Pending`] state.
    ///
    /// # Arguments
    /// * `source` - Source URL or path label.
    /// * `path` - Local workspace directory path.
    fn new_report(source: String, path: PathBuf) -> Self;

    /// Update the [`CloneStatus`] on this report.
    ///
    /// # Arguments
    /// * `status` - New clone status value.
    fn set_clone_status(&mut self, status: CloneStatus);
}

impl BatchReportItem for RepoReport {
    fn new_report(source: String, path: PathBuf) -> Self {
        Self::new(source, path)
    }

    fn set_clone_status(&mut self, status: CloneStatus) {
        self.clone_status = status;
    }
}

impl BatchReportItem for RefitReport {
    fn new_report(source: String, path: PathBuf) -> Self {
        Self::new(source, path)
    }

    fn set_clone_status(&mut self, status: CloneStatus) {
        self.clone_status = status;
    }
}

impl BatchReportItem for LaunchReport {
    fn new_report(source: String, path: PathBuf) -> Self {
        Self::new(source, path)
    }

    fn set_clone_status(&mut self, status: CloneStatus) {
        self.clone_status = status;
    }
}

/// Load targets and create the clone directory when remote targets are present.
///
/// # Arguments
/// * `verb` - Action name used in the empty-target message (`"audit"`, `"refit"`, `"launch"`).
/// * `source` - Resolved repository source.
/// * `clone_output` - Directory where remote repositories are cloned.
async fn prepare_batch_targets(
    verb: &str,
    source: BatchSource,
    clone_output: &Path,
) -> CliResult<Option<Vec<RepoTarget>>> {
    let targets = load_repo_targets(source, clone_output).await?;
    if targets.is_empty() {
        println!("No repositories found to {verb}.");
        return Ok(None);
    }

    if targets
        .iter()
        .any(|target| matches!(target, RepoTarget::Clone { .. }))
    {
        tokio::fs::create_dir_all(clone_output).await?;
    }

    Ok(Some(targets))
}

/// Generic concurrent batch runner shared by `audit`, `refit`, and `launch`.
///
/// # Arguments
/// * `verb` - Action name used in the empty-target message (`"audit"`, `"refit"`, `"launch"`).
/// * `source` - Resolved repository source.
/// * `clone_output` - Directory where remote repositories are cloned.
/// * `concurrency` - Maximum concurrent worker tasks.
/// * `worker` - Per-target async worker closure.
async fn execute_batch_targets<R, F, Fut>(
    verb: &str,
    source: BatchSource,
    clone_output: PathBuf,
    concurrency: usize,
    worker: F,
) -> CliResult<Option<Vec<R>>>
where
    R: BatchReportItem,
    F: Fn(RepoTarget) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
    let Some(targets) = prepare_batch_targets(verb, source, &clone_output).await? else {
        return Ok(None);
    };

    let concurrency = concurrency.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let worker = Arc::new(worker);
    let mut tasks = JoinSet::new();

    for target in targets {
        let permit = Arc::clone(&semaphore).acquire_owned().await?;
        let worker = Arc::clone(&worker);
        tasks.spawn(async move {
            let _permit = permit;
            worker(target).await
        });
    }

    let mut reports = Vec::with_capacity(tasks.len());
    while let Some(result) = tasks.join_next().await {
        reports.push(result?);
    }

    Ok(Some(reports))
}

/// Prepare the filesystem workspace for `target` and return `(source_label, path, CloneStatus)`.
///
/// # Arguments
/// * `target` - Clone or local target descriptor.
async fn prepare_target_workspace(target: RepoTarget) -> (String, PathBuf, CloneStatus) {
    match target {
        RepoTarget::Clone { url, dest } => {
            if dest.exists() {
                let msg = format!("destination exists: {}", dest.display());
                return (url, dest, CloneStatus::Failed(msg));
            }
            let status = match clone_repo(&url, &dest).await {
                Ok(()) => CloneStatus::Cloned,
                Err(err) => CloneStatus::Failed(err.to_string()),
            };
            (url, dest, status)
        }
        RepoTarget::Local { path } => {
            let source = path.display().to_string();
            if !path.is_dir() {
                let msg = format!("path not found: {}", path.display());
                return (source, path, CloneStatus::Failed(msg));
            }
            (source, path, CloneStatus::Local)
        }
    }
}

/// Prepare a single [`RepoTarget`] (cloning or verifying local directory) and run `populate`.
///
/// # Arguments
/// * `target` - Clone or local target descriptor.
/// * `populate` - Callback invoked when the repository workspace is ready.
async fn process_repo_target<R, F>(target: RepoTarget, populate: F) -> R
where
    R: BatchReportItem,
    F: FnOnce(&mut R),
{
    let (source, path, status) = prepare_target_workspace(target).await;
    let mut report = R::new_report(source, path);
    let ready = matches!(status, CloneStatus::Cloned | CloneStatus::Local);
    report.set_clone_status(status);
    if ready {
        populate(&mut report);
    }
    report
}

/// Execute the `audit` / `batch` workflow across all resolved targets.
///
/// # Arguments
/// * `source` - Resolved repository source.
/// * `clone_output` - Clone output directory.
/// * `concurrency` - Max concurrency.
/// * `mechanic_ids` - Mechanic identifiers to run.
/// * `report` - Report output arguments.
async fn run_audit(
    source: BatchSource,
    clone_output: PathBuf,
    concurrency: usize,
    mechanic_ids: Vec<String>,
    report: OutputArgs,
) -> CliResult<()> {
    let mechanics = Arc::new(build_mechanics(&mechanic_ids)?);
    let worker = move |target| {
        let mechanics = Arc::clone(&mechanics);
        async move {
            process_repo_target(target, |rep: &mut RepoReport| {
                populate_audit(rep, mechanics.as_ref());
            })
            .await
        }
    };
    let Some(reports) =
        execute_batch_targets("audit", source, clone_output, concurrency, worker).await?
    else {
        return Ok(());
    };

    emit_audit_reports(&reports, &report)
}

/// Execute the `refit` workflow across all resolved targets.
///
/// # Arguments
/// * `source` - Resolved repository source.
/// * `clone_output` - Clone output directory.
/// * `concurrency` - Max concurrency.
/// * `mechanic_ids` - Mechanic identifiers to run.
/// * `report` - Report output arguments.
/// * `apply` - Whether to apply fixes (`true`) or run in dry-run mode (`false`).
async fn run_refit(
    source: BatchSource,
    clone_output: PathBuf,
    concurrency: usize,
    mechanic_ids: Vec<String>,
    report: OutputArgs,
    apply: bool,
) -> CliResult<()> {
    let mechanics = Arc::new(build_mechanics(&mechanic_ids)?);
    let worker = move |target| {
        let mechanics = Arc::clone(&mechanics);
        async move {
            process_repo_target(target, |rep: &mut RefitReport| {
                populate_refit(rep, mechanics.as_ref(), apply);
            })
            .await
        }
    };
    let Some(reports) =
        execute_batch_targets("refit", source, clone_output, concurrency, worker).await?
    else {
        return Ok(());
    };

    emit_refit_reports(&reports, &report)
}

/// Execute the `launch` workflow across all resolved targets.
///
/// # Arguments
/// * `source` - Resolved repository source.
/// * `clone_output` - Clone output directory.
/// * `concurrency` - Max concurrency.
/// * `report` - Report output arguments.
async fn run_launch(
    source: BatchSource,
    clone_output: PathBuf,
    concurrency: usize,
    report: OutputArgs,
) -> CliResult<()> {
    let worker = |target| async move { process_repo_target(target, populate_launch).await };
    let Some(reports) =
        execute_batch_targets("launch", source, clone_output, concurrency, worker).await?
    else {
        return Ok(());
    };

    emit_launch_reports(&reports, &report)
}

/// Resolve [`RepoSourceArgs`] into a validated [`BatchSource`].
///
/// # Arguments
/// * `source` - Raw CLI source flags.
fn resolve_source_args(source: &RepoSourceArgs) -> CliResult<BatchSource> {
    if let Some(file) = source.file.clone() {
        return Ok(BatchSource::File(file));
    }
    if let Some(url) = source.url.clone() {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(CliError::EmptyUrl);
        }
        return Ok(BatchSource::Url(trimmed.to_string()));
    }
    if let Some(dir) = source.dir.clone() {
        return Ok(BatchSource::Dir(dir));
    }
    if let Some(path) = source.path.clone() {
        return Ok(BatchSource::Path(path));
    }
    Err(CliError::MissingSource)
}

/// Load non-empty, non-comment repository URLs from a text file at `path`.
///
/// # Arguments
/// * `path` - File path containing newline-delimited URLs.
async fn load_repo_urls(path: &Path) -> CliResult<Vec<String>> {
    let contents = tokio::fs::read_to_string(path).await?;
    let urls = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect();
    Ok(urls)
}

/// List non-hidden immediate subdirectories inside `path`.
///
/// # Arguments
/// * `path` - Parent directory containing local repositories.
async fn load_repo_paths_from_dir(path: &Path) -> CliResult<Vec<PathBuf>> {
    let mut entries = tokio::fs::read_dir(path).await?;
    let mut repos = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        if !file_type.is_dir() {
            continue;
        }
        let entry_path = entry.path();
        if is_hidden_path(&entry_path) {
            continue;
        }
        repos.push(entry_path);
    }
    repos.sort();
    Ok(repos)
}

/// Expand a [`BatchSource`] into concrete [`RepoTarget`] items.
///
/// # Arguments
/// * `source` - Resolved repository source.
/// * `output` - Root directory for cloned repositories.
async fn load_repo_targets(source: BatchSource, output: &Path) -> CliResult<Vec<RepoTarget>> {
    match source {
        BatchSource::File(file) => {
            let urls = load_repo_urls(&file).await?;
            Ok(urls
                .into_iter()
                .map(|url| RepoTarget::Clone {
                    dest: output.join(repo_dir_name(&url)),
                    url,
                })
                .collect())
        }
        BatchSource::Url(url) => Ok(vec![RepoTarget::Clone {
            dest: output.join(repo_dir_name(&url)),
            url,
        }]),
        BatchSource::Dir(dir) => {
            let paths = load_repo_paths_from_dir(&dir).await?;
            Ok(paths
                .into_iter()
                .map(|path| RepoTarget::Local { path })
                .collect())
        }
        BatchSource::Path(path) => Ok(vec![RepoTarget::Local { path }]),
    }
}

/// Validated input source for batch operations.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BatchSource {
    /// Text file containing repository URLs.
    File(PathBuf),
    /// Single repository URL.
    Url(String),
    /// Directory of local repositories.
    Dir(PathBuf),
    /// Single local repository path.
    Path(PathBuf),
}

/// Concrete repository target for a worker task.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RepoTarget {
    /// Remote repository to clone into `dest`.
    Clone {
        /// Remote Git URL.
        url: String,
        /// Destination directory path.
        dest: PathBuf,
    },
    /// Existing local repository directory.
    Local {
        /// Local repository path.
        path: PathBuf,
    },
}

/// Populate `report` with language statistics and mechanic audit violations.
///
/// # Arguments
/// * `report` - Target audit report to populate.
/// * `mechanics` - Mechanics to run against `report.path`.
fn populate_audit(report: &mut RepoReport, mechanics: &[Arc<dyn Mechanic + Send + Sync>]) {
    match inspect_language_stats(&report.path) {
        Ok(stats) => report.language_stats = Some(stats),
        Err(err) => report.audit_errors.push(format!("language stats: {err}")),
    }

    for mechanic in mechanics {
        match mechanic.audit(&report.path) {
            Ok(mut violations) => report.violations.append(&mut violations),
            Err(err) => report
                .audit_errors
                .push(format!("mechanic {}: {err}", mechanic.id())),
        }
    }
}

/// Populate `report` with refit dry-run diffs or applied results.
///
/// # Arguments
/// * `report` - Target refit report to populate.
/// * `mechanics` - Mechanics to run against `report.path`.
/// * `apply` - Whether to apply fixes (`true`) or run in dry-run mode (`false`).
fn populate_refit(
    report: &mut RefitReport,
    mechanics: &[Arc<dyn Mechanic + Send + Sync>],
    apply: bool,
) {
    for mechanic in mechanics {
        if apply {
            match mechanic.apply(&report.path) {
                Ok(changed) => report.results.push(format!(
                    "{}: applied (changes: {})",
                    mechanic.id(),
                    if changed { "yes" } else { "no" }
                )),
                Err(err) => report
                    .errors
                    .push(format!("mechanic {}: {err}", mechanic.id())),
            }
        } else {
            match mechanic.dry_run(&report.path) {
                Ok(output) => {
                    let trimmed = output.trim();
                    if trimmed.is_empty() {
                        report
                            .results
                            .push(format!("{}: dry-run (no output)", mechanic.id()));
                    } else {
                        report.results.push(format!(
                            "{}: dry-run
{trimmed}",
                            mechanic.id()
                        ));
                    }
                }
                Err(err) => report
                    .errors
                    .push(format!("mechanic {}: {err}", mechanic.id())),
            }
        }
    }
}

/// Populate `report` with generated `Dockerfile` and `.gitlab-ci.yml` configurations.
///
/// # Arguments
/// * `report` - Target launch report to populate.
fn populate_launch(report: &mut LaunchReport) {
    let (dockerfile, ci_config) = generate_ci_config(&report.path);
    report.dockerfile = Some(dockerfile);
    report.ci_config = Some(ci_config);
}

/// Ambient Git environment variables that bind `git` to an outer repository or hook context.
const GIT_LOCAL_ENV_VARS: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_AUTHOR_DATE",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_NAME",
    "GIT_COMMON_DIR",
    "GIT_COMMITTER_DATE",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

/// Clone a Git repository from `url` into `dest`.
///
/// # Arguments
/// * `url` - Remote repository URL or local path.
/// * `dest` - Target directory to clone into.
async fn clone_repo(url: &str, dest: &Path) -> CliResult<()> {
    let mut cmd = Command::new("git");
    for key in GIT_LOCAL_ENV_VARS {
        cmd.env_remove(key);
    }
    cmd.arg("clone").arg(url).arg(dest);
    let status = cmd.status().await?;

    if status.success() {
        Ok(())
    } else {
        Err(CliError::GitCloneFailed {
            status: status.to_string(),
        })
    }
}

/// Inspect language distribution at `path` using [`TokeiInspector`].
///
/// # Arguments
/// * `path` - Repository directory path.
fn inspect_language_stats(path: &Path) -> shipshape_core::Result<LanguageDistribution> {
    let inspector = TokeiInspector::new(StdFileSystem::new());
    inspector.inspect(path)
}

/// Derive a clean directory name from a Git repository URL.
///
/// # Arguments
/// * `url` - HTTPS or SSH Git repository URL.
fn repo_dir_name(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let last_segment = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let last_segment = last_segment.rsplit(':').next().unwrap_or(last_segment);
    last_segment.trim_end_matches(".git").to_string()
}

/// Format and emit audit reports according to `output`.
///
/// # Arguments
/// * `reports` - Audit reports to format.
/// * `output` - Output format and optional file destination.
fn emit_audit_reports(reports: &[RepoReport], output: &OutputArgs) -> CliResult<()> {
    let contents = match output.format {
        OutputFormat::Text => render_audit_text(reports),
        OutputFormat::Markdown => render_audit_markdown(reports),
        OutputFormat::Json => render_json(reports)?,
    };
    emit_output(output, contents)
}

/// Format and emit refit reports according to `output`.
///
/// # Arguments
/// * `reports` - Refit reports to format.
/// * `output` - Output format and optional file destination.
fn emit_refit_reports(reports: &[RefitReport], output: &OutputArgs) -> CliResult<()> {
    let contents = match output.format {
        OutputFormat::Text => render_refit_text(reports),
        OutputFormat::Markdown => render_refit_markdown(reports),
        OutputFormat::Json => render_json(reports)?,
    };
    emit_output(output, contents)
}

/// Format and emit launch reports according to `output`.
///
/// # Arguments
/// * `reports` - Launch reports to format.
/// * `output` - Output format and optional file destination.
fn emit_launch_reports(reports: &[LaunchReport], output: &OutputArgs) -> CliResult<()> {
    let contents = match output.format {
        OutputFormat::Text => render_launch_text(reports),
        OutputFormat::Markdown => render_launch_markdown(reports),
        OutputFormat::Json => render_json(reports)?,
    };
    emit_output(output, contents)
}

/// Write `contents` to `output.report_output` or standard output.
///
/// # Arguments
/// * `output` - Output destination settings.
/// * `contents` - Formatted report string.
fn emit_output(output: &OutputArgs, contents: String) -> CliResult<()> {
    if let Some(path) = &output.report_output {
        shipshape_core::FileSystem::write_file(&StdFileSystem::new(), path, &contents)?;
    } else {
        print!("{contents}");
    }
    Ok(())
}

/// Append the common `Source`, `Path`, and `Status` lines to `output`, returning `true`
/// if the repository is ready for detail rendering (`Cloned` or `Local`).
///
/// # Arguments
/// * `output` - Target text buffer.
/// * `source` - Repository source string.
/// * `path` - Repository workspace path.
/// * `status` - Clone status.
fn append_text_header(
    output: &mut String,
    source: &str,
    path: &Path,
    status: &CloneStatus,
) -> bool {
    output.push_str(&format!(
        "Source: {source}
"
    ));
    output.push_str(&format!(
        "Path: {}
",
        path.display()
    ));
    match status {
        CloneStatus::Cloned => {
            output.push_str(
                "Status: cloned
",
            );
            true
        }
        CloneStatus::Local => {
            output.push_str(
                "Status: local
",
            );
            true
        }
        CloneStatus::Failed(error) => {
            output.push_str(&format!(
                "Status: failed ({error})

"
            ));
            false
        }
        CloneStatus::Pending => {
            output.push_str(
                "Status: pending

",
            );
            false
        }
    }
}

/// Render audit reports as plain text.
///
/// # Arguments
/// * `reports` - Slice of audit reports.
fn render_audit_text(reports: &[RepoReport]) -> String {
    let mut output = String::new();
    for report in reports {
        if !append_text_header(
            &mut output,
            &report.source,
            &report.path,
            &report.clone_status,
        ) {
            continue;
        }

        match &report.language_stats {
            Some(stats) if stats.is_empty() => {
                output.push_str(
                    "Languages: none detected
",
                );
            }
            Some(stats) => {
                output.push_str(
                    "Languages:
",
                );
                for (language, percent) in format_language_stats(stats) {
                    output.push_str(&format!(
                        "- {language}: {percent:.2}%
"
                    ));
                }
            }
            None => {
                output.push_str(
                    "Languages: unavailable
",
                );
            }
        }

        if report.violations.is_empty() {
            output.push_str(
                "Violations: none
",
            );
        } else {
            output.push_str(
                "Violations:
",
            );
            for violation in &report.violations {
                output.push_str(&format!(
                    "- [{}] {}
",
                    violation.id, violation.message
                ));
            }
        }

        if !report.audit_errors.is_empty() {
            output.push_str("Audit errors:\n");
            for error in &report.audit_errors {
                output.push_str(&format!("- {error}\n"));
            }
        }

        output.push('\n');
    }
    output
}

/// Render refit reports as plain text.
///
/// # Arguments
/// * `reports` - Slice of refit reports.
fn render_refit_text(reports: &[RefitReport]) -> String {
    let mut output = String::new();
    for report in reports {
        if !append_text_header(
            &mut output,
            &report.source,
            &report.path,
            &report.clone_status,
        ) {
            continue;
        }

        if report.results.is_empty() {
            output.push_str("Refit results: none\n");
        } else {
            output.push_str("Refit results:\n");
            for result in &report.results {
                output.push_str(&format!("{result}\n"));
            }
        }

        if !report.errors.is_empty() {
            output.push_str("Refit errors:\n");
            for error in &report.errors {
                output.push_str(&format!("- {error}\n"));
            }
        }

        output.push('\n');
    }
    output
}

/// Render launch reports as plain text.
///
/// # Arguments
/// * `reports` - Slice of launch reports.
fn render_launch_text(reports: &[LaunchReport]) -> String {
    let mut output = String::new();
    for report in reports {
        if !append_text_header(
            &mut output,
            &report.source,
            &report.path,
            &report.clone_status,
        ) {
            continue;
        }

        match &report.dockerfile {
            Some(contents) => {
                output.push_str("Dockerfile:\n");
                output.push_str(&format!("{contents}\n"));
            }
            None => {
                output.push_str("Dockerfile: unavailable\n");
            }
        }

        match &report.ci_config {
            Some(contents) => {
                output.push_str(".gitlab-ci.yml:\n");
                output.push_str(&format!("{contents}\n"));
            }
            None => {
                output.push_str("CI config: unavailable\n");
            }
        }

        if !report.errors.is_empty() {
            output.push_str("Launch errors:\n");
            for error in &report.errors {
                output.push_str(&format!("- {error}\n"));
            }
        }

        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use shipshape_core::{FileSystem as _, Violation};
    use std::collections::BTreeMap;
    use std::process::Command as StdCommand;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Atomic counter for unique test directory names.
    static UNIQUE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Generate a unique directory name for tests.
    fn unique_dir_name() -> PathBuf {
        let pid = std::process::id();
        let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        PathBuf::from(format!("shipshape_cli_test_{pid}_{counter}"))
    }

    #[test]
    fn repo_dir_name_handles_https_and_ssh() {
        super::main();
        assert_eq!(repo_dir_name("https://github.com/org/repo.git"), "repo");
        assert_eq!(repo_dir_name("git@github.com:org/repo.git"), "repo");
    }

    #[test]
    fn batch_report_item_trait_covers_all_report_types() {
        let mut r1 = RepoReport::new_report("s".to_string(), PathBuf::from("."));
        r1.set_clone_status(CloneStatus::Local);
        assert_eq!(r1.clone_status, CloneStatus::Local);

        let mut r2 = RefitReport::new_report("s".to_string(), PathBuf::from("."));
        r2.set_clone_status(CloneStatus::Local);
        assert_eq!(r2.clone_status, CloneStatus::Local);

        let mut r3 = LaunchReport::new_report("s".to_string(), PathBuf::from("."));
        r3.set_clone_status(CloneStatus::Local);
        assert_eq!(r3.clone_status, CloneStatus::Local);
    }

    #[test]
    fn resolve_source_handles_all_variants_and_errors() -> CliResult<()> {
        let file_args = RepoSourceArgs {
            file: Some(PathBuf::from("repos.txt")),
            url: Some("https://example.com/repo.git".to_string()),
            dir: None,
            path: None,
        };
        assert_eq!(
            resolve_source_args(&file_args)?,
            BatchSource::File(PathBuf::from("repos.txt"))
        );

        let url_args = RepoSourceArgs {
            file: None,
            url: Some(" https://example.com/repo.git ".to_string()),
            dir: None,
            path: None,
        };
        assert_eq!(
            resolve_source_args(&url_args)?,
            BatchSource::Url("https://example.com/repo.git".to_string())
        );

        let dir_args = RepoSourceArgs {
            file: None,
            url: None,
            dir: Some(PathBuf::from("/tmp/dir")),
            path: None,
        };
        assert_eq!(
            resolve_source_args(&dir_args)?,
            BatchSource::Dir(PathBuf::from("/tmp/dir"))
        );

        let path_args = RepoSourceArgs {
            file: None,
            url: None,
            dir: None,
            path: Some(PathBuf::from("/tmp/path")),
        };
        assert_eq!(
            resolve_source_args(&path_args)?,
            BatchSource::Path(PathBuf::from("/tmp/path"))
        );

        let empty_url = RepoSourceArgs {
            file: None,
            url: Some("   ".to_string()),
            dir: None,
            path: None,
        };
        assert!(matches!(
            resolve_source_args(&empty_url),
            Err(CliError::EmptyUrl)
        ));

        let missing = RepoSourceArgs {
            file: None,
            url: None,
            dir: None,
            path: None,
        };
        assert!(matches!(
            resolve_source_args(&missing),
            Err(CliError::MissingSource)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn load_repo_paths_and_urls_and_targets() -> CliResult<()> {
        let root = std::env::temp_dir().join(unique_dir_name());
        let batch_dir = root.join("batch");
        let repo_a = batch_dir.join("repo-a");
        let repo_b = batch_dir.join("repo-b");
        let hidden = batch_dir.join(".hidden");
        std::fs::create_dir_all(&repo_a)?;
        std::fs::create_dir_all(&repo_b)?;
        std::fs::create_dir_all(&hidden)?;
        let notes = batch_dir.join("notes.txt");
        std::fs::write(&notes, "data")?;

        let repos = load_repo_paths_from_dir(&batch_dir).await?;
        assert_eq!(repos, vec![repo_a.clone(), repo_b]);

        let file_path = root.join("repos.txt");
        let list_body = "# comment\n\nhttps://example.com/a.git\n  \nhttps://example.com/b.git\n";
        std::fs::write(&file_path, list_body)?;
        let urls = load_repo_urls(&file_path).await?;
        assert_eq!(
            urls,
            vec!["https://example.com/a.git", "https://example.com/b.git"]
        );

        let output = root.join("out");
        let file_targets = load_repo_targets(BatchSource::File(file_path), &output).await?;
        assert_eq!(file_targets.len(), 2);
        let url_source = BatchSource::Url("https://example.com/b.git".to_string());
        let url_targets = load_repo_targets(url_source, &output).await?;
        assert_eq!(url_targets.len(), 1);
        let dir_targets = load_repo_targets(BatchSource::Dir(batch_dir), &output).await?;
        assert_eq!(dir_targets.len(), 2);
        let path_targets = load_repo_targets(BatchSource::Path(repo_a), &output).await?;
        assert_eq!(path_targets.len(), 1);

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[tokio::test]
    async fn process_repo_target_handles_missing_local_existing_dest_and_cloning() -> CliResult<()>
    {
        let missing = std::env::temp_dir().join(unique_dir_name());
        let local_fail: LaunchReport =
            process_repo_target(RepoTarget::Local { path: missing }, populate_launch).await;
        assert!(matches!(local_fail.clone_status, CloneStatus::Failed(_)));

        let existing_dest = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&existing_dest)?;
        let existing_target = RepoTarget::Clone {
            url: "https://example.com/repo.git".to_string(),
            dest: existing_dest.clone(),
        };
        let dest_fail: LaunchReport = process_repo_target(existing_target, populate_launch).await;
        assert!(matches!(dest_fail.clone_status, CloneStatus::Failed(_)));
        std::fs::remove_dir_all(&existing_dest)?;

        // Successful clone & failed clone
        let source = init_git_repo()?;
        let source_str = source.to_string_lossy().to_string();
        let dest = std::env::temp_dir().join(unique_dir_name());
        let clone_target = RepoTarget::Clone {
            url: source_str,
            dest: dest.clone(),
        };
        let cloned: LaunchReport = process_repo_target(clone_target, populate_launch).await;
        assert_eq!(cloned.clone_status, CloneStatus::Cloned);
        assert!(cloned.dockerfile.is_some());

        let bad_dest = std::env::temp_dir().join(unique_dir_name());
        let missing_repo = std::env::temp_dir().join(unique_dir_name());
        let missing_str = missing_repo.to_string_lossy().to_string();
        let bad_target = RepoTarget::Clone {
            url: missing_str,
            dest: bad_dest,
        };
        let clone_fail: LaunchReport = process_repo_target(bad_target, populate_launch).await;
        assert!(matches!(clone_fail.clone_status, CloneStatus::Failed(_)));

        std::fs::remove_dir_all(&source)?;
        std::fs::remove_dir_all(&dest)?;
        Ok(())
    }

    #[test]
    fn populate_audit_refit_and_launch_cover_all_mechanic_branches() -> CliResult<()> {
        let repo = temp_repo_with_file("src/main.rs", "fn main() {}\n")?;
        let mut audit_report = RepoReport::new("local".to_string(), repo.clone());
        let mechanics: Vec<Arc<dyn Mechanic + Send + Sync>> = vec![
            Arc::new(TestMechanic {
                id: "ok-mech",
                audit_result: Ok(vec![Violation {
                    id: "ok-mech".to_string(),
                    message: "violation".to_string(),
                }]),
                dry_run_result: Ok(String::new()),
                apply_result: Ok(true),
            }),
            Arc::new(TestMechanic {
                id: "diff-mech",
                audit_result: Ok(Vec::new()),
                dry_run_result: Ok("diff".to_string()),
                apply_result: Ok(false),
            }),
            Arc::new(TestMechanic {
                id: "err-mech",
                audit_result: Err("audit boom".to_string()),
                dry_run_result: Err("dry boom".to_string()),
                apply_result: Err("apply boom".to_string()),
            }),
        ];

        populate_audit(&mut audit_report, &mechanics);
        assert!(audit_report.language_stats.is_some());
        assert_eq!(audit_report.violations.len(), 1);
        assert_eq!(audit_report.audit_errors.len(), 1);

        let mut missing_audit = RepoReport::new(
            "missing".to_string(),
            std::env::temp_dir().join(unique_dir_name()),
        );
        populate_audit(&mut missing_audit, &[]);
        assert!(!missing_audit.audit_errors.is_empty());

        let mut apply_report = RefitReport::new("local".to_string(), repo.clone());
        populate_refit(&mut apply_report, &mechanics, true);
        assert!(
            apply_report
                .results
                .iter()
                .any(|r| r.contains("changes: yes"))
        );
        assert!(
            apply_report
                .results
                .iter()
                .any(|r| r.contains("changes: no"))
        );
        assert!(apply_report.errors.iter().any(|e| e.contains("apply boom")));

        let mut dry_report = RefitReport::new("local".to_string(), repo.clone());
        populate_refit(&mut dry_report, &mechanics, false);
        assert!(
            dry_report
                .results
                .iter()
                .any(|r| r.contains("dry-run (no output)"))
        );
        assert!(dry_report.results.iter().any(|r| r.contains("diff")));
        assert!(dry_report.errors.iter().any(|e| e.contains("dry boom")));

        let mut launch_report = LaunchReport::new("local".to_string(), repo.clone());
        populate_launch(&mut launch_report);
        assert!(launch_report.dockerfile.is_some());
        assert!(launch_report.ci_config.is_some());

        std::fs::remove_dir_all(&repo)?;
        Ok(())
    }

    #[test]
    fn render_text_functions_cover_all_status_and_content_branches() {
        let mut audit_a = RepoReport::new("cloned".to_string(), PathBuf::from("/tmp/a"));
        audit_a.clone_status = CloneStatus::Cloned;
        let mut stats = BTreeMap::new();
        stats.insert("Rust".to_string(), 80.0);
        audit_a.language_stats = Some(stats);
        audit_a.violations = vec![Violation {
            id: "v1".to_string(),
            message: "m1".to_string(),
        }];

        let mut audit_b = RepoReport::new("local".to_string(), PathBuf::from("/tmp/b"));
        audit_b.clone_status = CloneStatus::Local;
        audit_b.language_stats = Some(BTreeMap::new());
        audit_b.audit_errors = vec!["err".to_string()];

        let mut audit_c = RepoReport::new("none".to_string(), PathBuf::from("/tmp/c"));
        audit_c.clone_status = CloneStatus::Cloned;
        audit_c.language_stats = None;

        let mut audit_failed = RepoReport::new("failed".to_string(), PathBuf::from("/tmp/d"));
        audit_failed.clone_status = CloneStatus::Failed("oops".to_string());

        let mut audit_pending = RepoReport::new("pending".to_string(), PathBuf::from("/tmp/e"));
        audit_pending.clone_status = CloneStatus::Pending;

        let audit_text =
            render_audit_text(&[audit_a, audit_b, audit_c, audit_failed, audit_pending]);
        assert!(audit_text.contains("Status: cloned"));
        assert!(audit_text.contains("Status: local"));
        assert!(audit_text.contains("Status: failed (oops)"));
        assert!(audit_text.contains("Status: pending"));
        assert!(audit_text.contains("Languages: none detected"));
        assert!(audit_text.contains("Languages: unavailable"));
        assert!(audit_text.contains("Violations: none"));
        assert!(audit_text.contains("[v1] m1"));
        assert!(audit_text.contains("Audit errors:"));

        let mut refit_a = RefitReport::new("cloned".to_string(), PathBuf::from("/tmp/a"));
        refit_a.clone_status = CloneStatus::Cloned;
        refit_a.results = vec!["apply: ok".to_string()];
        let mut refit_b = RefitReport::new("local".to_string(), PathBuf::from("/tmp/b"));
        refit_b.clone_status = CloneStatus::Local;
        refit_b.errors = vec!["err".to_string()];
        let mut refit_failed = RefitReport::new("failed".to_string(), PathBuf::from("/tmp/c"));
        refit_failed.clone_status = CloneStatus::Failed("oops".to_string());
        let refit_text = render_refit_text(&[refit_a, refit_b, refit_failed]);
        assert!(refit_text.contains("apply: ok"));
        assert!(refit_text.contains("Refit results: none"));
        assert!(refit_text.contains("Refit errors:"));

        let mut launch_a = LaunchReport::new("cloned".to_string(), PathBuf::from("/tmp/a"));
        launch_a.clone_status = CloneStatus::Cloned;
        launch_a.dockerfile = Some("FROM rust".to_string());
        launch_a.ci_config = Some("stages:".to_string());
        let mut launch_b = LaunchReport::new("local".to_string(), PathBuf::from("/tmp/b"));
        launch_b.clone_status = CloneStatus::Local;
        launch_b.errors = vec!["err".to_string()];
        let mut launch_failed = LaunchReport::new("failed".to_string(), PathBuf::from("/tmp/c"));
        launch_failed.clone_status = CloneStatus::Failed("oops".to_string());
        let launch_text = render_launch_text(&[launch_a, launch_b, launch_failed]);
        assert!(launch_text.contains("FROM rust"));
        assert!(launch_text.contains("Dockerfile: unavailable"));
        assert!(launch_text.contains("CI config: unavailable"));
        assert!(launch_text.contains("Launch errors:"));
    }

    #[tokio::test]
    async fn emit_reports_and_run_cli_subcommands() -> CliResult<()> {
        let root = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&root)?;

        let audit_md = root.join("sub/audit.md");
        let audit_reps = [RepoReport::new("r".to_string(), PathBuf::from("/tmp/r"))];
        let md_out = OutputArgs {
            format: OutputFormat::Markdown,
            report_output: Some(audit_md.clone()),
        };
        emit_audit_reports(&audit_reps, &md_out)?;
        assert!(std::fs::read_to_string(&audit_md)?.contains("# ShipShape Audit Report"));

        let json_out = OutputArgs {
            format: OutputFormat::Json,
            report_output: Some(root.join("sub/audit.json")),
        };
        emit_audit_reports(&audit_reps, &json_out)?;

        let refit_reps = [RefitReport::new("r".to_string(), PathBuf::from("/tmp/r"))];
        let refit_md_out = OutputArgs {
            format: OutputFormat::Markdown,
            report_output: Some(root.join("sub/refit.md")),
        };
        emit_refit_reports(&refit_reps, &refit_md_out)?;
        let refit_json_out = OutputArgs {
            format: OutputFormat::Json,
            report_output: Some(root.join("sub/refit.json")),
        };
        emit_refit_reports(&refit_reps, &refit_json_out)?;

        let launch_reps = [LaunchReport::new("r".to_string(), PathBuf::from("/tmp/r"))];
        let launch_md_out = OutputArgs {
            format: OutputFormat::Markdown,
            report_output: Some(root.join("sub/launch.md")),
        };
        emit_launch_reports(&launch_reps, &launch_md_out)?;
        let launch_json_out = OutputArgs {
            format: OutputFormat::Json,
            report_output: Some(root.join("sub/launch.json")),
        };
        emit_launch_reports(&launch_reps, &launch_json_out)?;

        // Exercise empty directory for audit, refit, and launch
        let empty_dir = root.join("empty");
        std::fs::create_dir_all(&empty_dir)?;
        let empty_str = empty_dir.to_string_lossy().to_string();
        run_cli_from_iter(&["shipshape", "batch", "--dir", &empty_str][..]).await?;
        run_cli_from_iter(&["shipshape", "refit", "--dir", &empty_str][..]).await?;
        run_cli_from_iter(&["shipshape", "launch", "--dir", &empty_str][..]).await?;

        // Exercise populated local repository and remote clone via CLI parser
        let git_repo = init_git_repo()?;
        let git_repo_str = git_repo.to_string_lossy().to_string();
        let clone_out = root.join("clones");
        let clone_out_str = clone_out.to_string_lossy().to_string();
        let audit_args = [
            "shipshape",
            "audit",
            "--url",
            &git_repo_str,
            "--output",
            &clone_out_str,
        ];
        run_cli_from_iter(&audit_args[..]).await?;

        let refit_args = [
            "shipshape",
            "refit",
            "--path",
            &git_repo_str,
            "--mechanic",
            "noop",
        ];
        run_cli_from_iter(&refit_args[..]).await?;
        run_cli_from_iter(&["shipshape", "launch", "--path", &git_repo_str][..]).await?;

        // Exercise login error via CLI parser (empty server-url)
        let login_args = ["shipshape", "login", "--server-url", "   "];
        let login_res = run_cli_from_iter(&login_args[..]).await;
        assert!(matches!(login_res, Err(CliError::EmptyServerUrl)));

        std::fs::remove_dir_all(&git_repo)?;
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// Create a temporary repository containing `rel_path` with `contents`.
    fn temp_repo_with_file(rel_path: &str, contents: &str) -> CliResult<PathBuf> {
        let root = std::env::temp_dir().join(unique_dir_name());
        let file_path = root.join(rel_path);
        StdFileSystem::new().write_file(&file_path, contents)?;
        Ok(root)
    }

    /// Run a local `git` command in `root` with ambient repository environment variables removed.
    fn run_test_git(root: &Path, args: &[&str]) -> CliResult<()> {
        let mut cmd = StdCommand::new("git");
        for key in GIT_LOCAL_ENV_VARS {
            cmd.env_remove(key);
        }
        let _ = cmd.args(args).current_dir(root).status()?;
        Ok(())
    }

    /// Initialize a local Git repository with a single commit for clone tests.
    fn init_git_repo() -> CliResult<PathBuf> {
        let root = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&root)?;
        run_test_git(&root, &["init", "-q"])?;
        let readme = root.join("README.md");
        std::fs::write(&readme, "shipshape")?;
        run_test_git(&root, &["add", "."])?;
        let commit_args = [
            "-c",
            "user.name=ShipShape",
            "-c",
            "user.email=shipshape@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "init",
        ];
        run_test_git(&root, &commit_args)?;
        Ok(root)
    }

    /// Configurable mechanic stub for unit tests.
    struct TestMechanic {
        /// Mechanic identifier.
        id: &'static str,
        /// Preset `audit` result.
        audit_result: Result<Vec<Violation>, String>,
        /// Preset `dry_run` result.
        dry_run_result: Result<String, String>,
        /// Preset `apply` result.
        apply_result: Result<bool, String>,
    }

    impl Mechanic for TestMechanic {
        fn id(&self) -> &str {
            self.id
        }

        fn audit(&self, _path: &Path) -> Result<Vec<Violation>, ShipShapeError> {
            self.audit_result.clone().map_err(ShipShapeError::Other)
        }

        fn dry_run(&self, _path: &Path) -> Result<String, ShipShapeError> {
            self.dry_run_result.clone().map_err(ShipShapeError::Other)
        }

        fn apply(&self, _path: &Path) -> Result<bool, ShipShapeError> {
            self.apply_result.clone().map_err(ShipShapeError::Other)
        }
    }
}
