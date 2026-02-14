#![deny(missing_docs)]
//! ShipShape command-line interface.
//!
//! Provides batch audit, refit, and launch workflows for repositories.

mod auth;

use auth::LoginArgs;
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use shipshape_core::{
    CloneStatus, LanguageDistribution, LaunchReport, Mechanic, RefitReport, RepoReport,
    StdFileSystem, TokeiInspector, build_mechanics, format_language_stats, generate_ci_config,
    render_audit_markdown, render_json, render_launch_markdown, render_refit_markdown,
};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub(crate) type CliResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Parser)]
#[command(name = "shipshape", version, about = "ShipShape CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

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

#[derive(Args, Clone)]
struct CloneArgs {
    /// Output directory to clone into.
    #[arg(short, long, default_value = "shipshape-batch")]
    output: PathBuf,
    /// Maximum number of concurrent clones.
    #[arg(short = 'j', long, default_value_t = 5)]
    concurrency: usize,
}

#[derive(Args, Clone)]
struct MechanicArgs {
    /// Mechanic IDs to run (repeatable or comma-separated).
    #[arg(long, value_delimiter = ',')]
    mechanic: Vec<String>,
}

#[derive(Args, Clone)]
struct OutputArgs {
    /// Output format for report data.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Write the report to a file instead of stdout.
    #[arg(long = "report-output")]
    report_output: Option<PathBuf>,
}

#[derive(ValueEnum, Copy, Clone, Debug, Eq, PartialEq)]
enum OutputFormat {
    Text,
    Json,
    Markdown,
}

#[derive(Subcommand)]
enum Commands {
    /// Clone and audit repositories from a URL, file, directory, or local path.
    Batch {
        #[command(flatten)]
        source: RepoSourceArgs,
        #[command(flatten)]
        clone: CloneArgs,
        #[command(flatten)]
        mechanics: MechanicArgs,
        #[command(flatten)]
        report: OutputArgs,
    },
    /// Audit repositories from a URL, file, directory, or local path.
    Audit {
        #[command(flatten)]
        source: RepoSourceArgs,
        #[command(flatten)]
        clone: CloneArgs,
        #[command(flatten)]
        mechanics: MechanicArgs,
        #[command(flatten)]
        report: OutputArgs,
    },
    /// Run refit mechanics in dry-run mode or apply fixes.
    Refit {
        #[command(flatten)]
        source: RepoSourceArgs,
        #[command(flatten)]
        clone: CloneArgs,
        #[command(flatten)]
        mechanics: MechanicArgs,
        #[command(flatten)]
        report: OutputArgs,
        /// Apply fixes instead of dry-run output.
        #[arg(long)]
        apply: bool,
    },
    /// Generate launch (CI) configs for repositories.
    Launch {
        #[command(flatten)]
        source: RepoSourceArgs,
        #[command(flatten)]
        clone: CloneArgs,
        #[command(flatten)]
        report: OutputArgs,
    },
    /// Authenticate the CLI via the GitHub device flow.
    Login(LoginArgs),
}

#[cfg(not(test))]
#[tokio::main]
async fn main() -> CliResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Batch {
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
            .await?
        }
        Commands::Audit {
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
            .await?
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
            .await?
        }
        Commands::Launch {
            source,
            clone,
            report,
        } => {
            let source = resolve_source_args(&source)?;
            run_launch(source, clone.output, clone.concurrency, report).await?
        }
        Commands::Login(args) => {
            auth::run_login(args).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
fn main() {}

async fn run_audit(
    source: BatchSource,
    clone_output: PathBuf,
    concurrency: usize,
    mechanic_ids: Vec<String>,
    report: OutputArgs,
) -> CliResult<()> {
    let targets = load_repo_targets(source, &clone_output).await?;
    if targets.is_empty() {
        println!("No repositories found to audit.");
        return Ok(());
    }

    if targets
        .iter()
        .any(|target| matches!(target, RepoTarget::Clone { .. }))
    {
        tokio::fs::create_dir_all(&clone_output).await?;
    }
    let mechanics = Arc::new(build_mechanics(&mechanic_ids)?);
    let concurrency = if concurrency == 0 { 1 } else { concurrency };
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut tasks = JoinSet::new();

    for target in targets {
        let permit = semaphore.clone().acquire_owned().await?;
        let mechanics = mechanics.clone();
        tasks.spawn(async move {
            let _permit = permit;
            audit_target(target, mechanics).await
        });
    }

    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(report) => reports.push(report),
            Err(err) => reports.push(repo_report_from_task_error(err)),
        }
    }

    emit_audit_reports(&reports, &report).await?;

    Ok(())
}

async fn run_refit(
    source: BatchSource,
    clone_output: PathBuf,
    concurrency: usize,
    mechanic_ids: Vec<String>,
    report: OutputArgs,
    apply: bool,
) -> CliResult<()> {
    let targets = load_repo_targets(source, &clone_output).await?;
    if targets.is_empty() {
        println!("No repositories found to refit.");
        return Ok(());
    }

    if targets
        .iter()
        .any(|target| matches!(target, RepoTarget::Clone { .. }))
    {
        tokio::fs::create_dir_all(&clone_output).await?;
    }
    let mechanics = Arc::new(build_mechanics(&mechanic_ids)?);
    let concurrency = if concurrency == 0 { 1 } else { concurrency };
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut tasks = JoinSet::new();

    for target in targets {
        let permit = semaphore.clone().acquire_owned().await?;
        let mechanics = mechanics.clone();
        tasks.spawn(async move {
            let _permit = permit;
            refit_target(target, mechanics, apply).await
        });
    }

    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(report) => reports.push(report),
            Err(err) => reports.push(refit_report_from_task_error(err)),
        }
    }

    emit_refit_reports(&reports, &report).await?;

    Ok(())
}

async fn run_launch(
    source: BatchSource,
    clone_output: PathBuf,
    concurrency: usize,
    report: OutputArgs,
) -> CliResult<()> {
    let targets = load_repo_targets(source, &clone_output).await?;
    if targets.is_empty() {
        println!("No repositories found to launch.");
        return Ok(());
    }

    if targets
        .iter()
        .any(|target| matches!(target, RepoTarget::Clone { .. }))
    {
        tokio::fs::create_dir_all(&clone_output).await?;
    }
    let concurrency = if concurrency == 0 { 1 } else { concurrency };
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut tasks = JoinSet::new();

    for target in targets {
        let permit = semaphore.clone().acquire_owned().await?;
        tasks.spawn(async move {
            let _permit = permit;
            launch_target(target).await
        });
    }

    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(report) => reports.push(report),
            Err(err) => reports.push(launch_report_from_task_error(err)),
        }
    }

    emit_launch_reports(&reports, &report).await?;

    Ok(())
}

fn resolve_source_args(source: &RepoSourceArgs) -> CliResult<BatchSource> {
    if let Some(file) = source.file.clone() {
        return Ok(BatchSource::File(file));
    }
    if let Some(url) = source.url.clone() {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err("url cannot be empty".into());
        }
        return Ok(BatchSource::Url(trimmed.to_string()));
    }
    if let Some(dir) = source.dir.clone() {
        return Ok(BatchSource::Dir(dir));
    }
    if let Some(path) = source.path.clone() {
        return Ok(BatchSource::Path(path));
    }
    Err("no repository source provided".into())
}

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

fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

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

enum BatchSource {
    File(PathBuf),
    Url(String),
    Dir(PathBuf),
    Path(PathBuf),
}

enum RepoTarget {
    Clone { url: String, dest: PathBuf },
    Local { path: PathBuf },
}

async fn audit_target(
    target: RepoTarget,
    mechanics: Arc<Vec<Arc<dyn Mechanic + Send + Sync>>>,
) -> RepoReport {
    match target {
        RepoTarget::Clone { url, dest } => clone_and_audit(url, dest, mechanics).await,
        RepoTarget::Local { path } => audit_local(path, mechanics).await,
    }
}

async fn refit_target(
    target: RepoTarget,
    mechanics: Arc<Vec<Arc<dyn Mechanic + Send + Sync>>>,
    apply: bool,
) -> RefitReport {
    match target {
        RepoTarget::Clone { url, dest } => clone_and_refit(url, dest, mechanics, apply).await,
        RepoTarget::Local { path } => refit_local(path, mechanics, apply).await,
    }
}

async fn launch_target(target: RepoTarget) -> LaunchReport {
    match target {
        RepoTarget::Clone { url, dest } => clone_and_launch(url, dest).await,
        RepoTarget::Local { path } => launch_local(path).await,
    }
}

async fn clone_and_audit(
    url: String,
    repo_dir: PathBuf,
    mechanics: Arc<Vec<Arc<dyn Mechanic + Send + Sync>>>,
) -> RepoReport {
    let mut report = RepoReport::new(url, repo_dir);

    if report.path.exists() {
        report.clone_status =
            CloneStatus::Failed(format!("destination exists: {}", report.path.display()));
        return report;
    }

    match clone_repo(&report.source, &report.path).await {
        Ok(()) => report.clone_status = CloneStatus::Cloned,
        Err(err) => {
            report.clone_status = CloneStatus::Failed(err.to_string());
            return report;
        }
    }

    populate_audit(&mut report, mechanics.as_ref());

    report
}

async fn clone_and_refit(
    url: String,
    repo_dir: PathBuf,
    mechanics: Arc<Vec<Arc<dyn Mechanic + Send + Sync>>>,
    apply: bool,
) -> RefitReport {
    let mut report = RefitReport::new(url, repo_dir);

    if report.path.exists() {
        report.clone_status =
            CloneStatus::Failed(format!("destination exists: {}", report.path.display()));
        return report;
    }

    match clone_repo(&report.source, &report.path).await {
        Ok(()) => report.clone_status = CloneStatus::Cloned,
        Err(err) => {
            report.clone_status = CloneStatus::Failed(err.to_string());
            return report;
        }
    }

    populate_refit(&mut report, mechanics.as_ref(), apply);

    report
}

async fn clone_and_launch(url: String, repo_dir: PathBuf) -> LaunchReport {
    let mut report = LaunchReport::new(url, repo_dir);

    if report.path.exists() {
        report.clone_status =
            CloneStatus::Failed(format!("destination exists: {}", report.path.display()));
        return report;
    }

    match clone_repo(&report.source, &report.path).await {
        Ok(()) => report.clone_status = CloneStatus::Cloned,
        Err(err) => {
            report.clone_status = CloneStatus::Failed(err.to_string());
            return report;
        }
    }

    populate_launch(&mut report);

    report
}

async fn audit_local(
    path: PathBuf,
    mechanics: Arc<Vec<Arc<dyn Mechanic + Send + Sync>>>,
) -> RepoReport {
    let mut report = RepoReport::new(path.display().to_string(), path);
    if !report.path.is_dir() {
        report.clone_status =
            CloneStatus::Failed(format!("path not found: {}", report.path.display()));
        return report;
    }

    report.clone_status = CloneStatus::Local;
    populate_audit(&mut report, mechanics.as_ref());
    report
}

async fn refit_local(
    path: PathBuf,
    mechanics: Arc<Vec<Arc<dyn Mechanic + Send + Sync>>>,
    apply: bool,
) -> RefitReport {
    let mut report = RefitReport::new(path.display().to_string(), path);
    if !report.path.is_dir() {
        report.clone_status =
            CloneStatus::Failed(format!("path not found: {}", report.path.display()));
        return report;
    }

    report.clone_status = CloneStatus::Local;
    populate_refit(&mut report, mechanics.as_ref(), apply);
    report
}

async fn launch_local(path: PathBuf) -> LaunchReport {
    let mut report = LaunchReport::new(path.display().to_string(), path);
    if !report.path.is_dir() {
        report.clone_status =
            CloneStatus::Failed(format!("path not found: {}", report.path.display()));
        return report;
    }

    report.clone_status = CloneStatus::Local;
    populate_launch(&mut report);
    report
}

fn populate_audit(report: &mut RepoReport, mechanics: &[Arc<dyn Mechanic + Send + Sync>]) {
    match inspect_language_stats(&report.path) {
        Ok(stats) => report.language_stats = Some(stats),
        Err(err) => report.audit_errors.push(format!("language stats: {err}")),
    }

    for mechanic in mechanics.iter() {
        match mechanic.audit(&report.path) {
            Ok(mut violations) => report.violations.append(&mut violations),
            Err(err) => report
                .audit_errors
                .push(format!("mechanic {}: {err}", mechanic.id())),
        }
    }
}

fn populate_refit(
    report: &mut RefitReport,
    mechanics: &[Arc<dyn Mechanic + Send + Sync>],
    apply: bool,
) {
    for mechanic in mechanics.iter() {
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
                        report
                            .results
                            .push(format!("{}: dry-run\n{}", mechanic.id(), trimmed));
                    }
                }
                Err(err) => report
                    .errors
                    .push(format!("mechanic {}: {err}", mechanic.id())),
            }
        }
    }
}

fn populate_launch(report: &mut LaunchReport) {
    let (dockerfile, ci_config) = generate_ci_config(&report.path);
    report.dockerfile = Some(dockerfile);
    report.ci_config = Some(ci_config);
}

async fn clone_repo(url: &str, dest: &Path) -> CliResult<()> {
    let status = Command::new("git")
        .arg("clone")
        .arg(url)
        .arg(dest)
        .status()
        .await?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("git clone failed with status {status}").into())
    }
}

fn inspect_language_stats(path: &Path) -> shipshape_core::Result<LanguageDistribution> {
    let inspector = TokeiInspector::new(StdFileSystem::new());
    inspector.inspect(path)
}

fn repo_dir_name(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let last_segment = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let last_segment = last_segment.rsplit(':').next().unwrap_or(last_segment);
    last_segment.trim_end_matches(".git").to_string()
}

fn repo_report_from_task_error(error: tokio::task::JoinError) -> RepoReport {
    RepoReport::failed("unknown".to_string(), PathBuf::from("."), error.to_string())
}

fn refit_report_from_task_error(error: tokio::task::JoinError) -> RefitReport {
    RefitReport::failed("unknown".to_string(), PathBuf::from("."), error.to_string())
}

fn launch_report_from_task_error(error: tokio::task::JoinError) -> LaunchReport {
    LaunchReport::failed("unknown".to_string(), PathBuf::from("."), error.to_string())
}

async fn emit_audit_reports(reports: &[RepoReport], output: &OutputArgs) -> CliResult<()> {
    let contents = match output.format {
        OutputFormat::Text => render_audit_text(reports),
        OutputFormat::Markdown => render_audit_markdown(reports),
        OutputFormat::Json => render_json(reports)?,
    };
    emit_output(output, contents).await
}

async fn emit_refit_reports(reports: &[RefitReport], output: &OutputArgs) -> CliResult<()> {
    let contents = match output.format {
        OutputFormat::Text => render_refit_text(reports),
        OutputFormat::Markdown => render_refit_markdown(reports),
        OutputFormat::Json => render_json(reports)?,
    };
    emit_output(output, contents).await
}

async fn emit_launch_reports(reports: &[LaunchReport], output: &OutputArgs) -> CliResult<()> {
    let contents = match output.format {
        OutputFormat::Text => render_launch_text(reports),
        OutputFormat::Markdown => render_launch_markdown(reports),
        OutputFormat::Json => render_json(reports)?,
    };
    emit_output(output, contents).await
}

async fn emit_output(output: &OutputArgs, contents: String) -> CliResult<()> {
    if let Some(path) = &output.report_output {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, contents).await?;
    } else {
        print!("{contents}");
    }
    Ok(())
}

fn render_audit_text(reports: &[RepoReport]) -> String {
    let mut output = String::new();
    for report in reports {
        let _ = writeln!(output, "Source: {}", report.source);
        let _ = writeln!(output, "Path: {}", report.path.display());
        match &report.clone_status {
            CloneStatus::Cloned => {
                let _ = writeln!(output, "Status: cloned");
            }
            CloneStatus::Local => {
                let _ = writeln!(output, "Status: local");
            }
            CloneStatus::Failed(error) => {
                let _ = writeln!(output, "Status: failed ({error})");
                let _ = writeln!(output);
                continue;
            }
            CloneStatus::Pending => {
                let _ = writeln!(output, "Status: pending");
                let _ = writeln!(output);
                continue;
            }
        }

        match &report.language_stats {
            Some(stats) if stats.is_empty() => {
                let _ = writeln!(output, "Languages: none detected");
            }
            Some(stats) => {
                let _ = writeln!(output, "Languages:");
                for (language, percent) in format_language_stats(stats) {
                    let _ = writeln!(output, "- {language}: {percent:.2}%");
                }
            }
            None => {
                let _ = writeln!(output, "Languages: unavailable");
            }
        }

        if !report.violations.is_empty() {
            let _ = writeln!(output, "Violations:");
            for violation in &report.violations {
                let _ = writeln!(output, "- [{}] {}", violation.id, violation.message);
            }
        } else {
            let _ = writeln!(output, "Violations: none");
        }

        if !report.audit_errors.is_empty() {
            let _ = writeln!(output, "Audit errors:");
            for error in &report.audit_errors {
                let _ = writeln!(output, "- {error}");
            }
        }

        let _ = writeln!(output);
    }
    output
}

fn render_refit_text(reports: &[RefitReport]) -> String {
    let mut output = String::new();
    for report in reports {
        let _ = writeln!(output, "Source: {}", report.source);
        let _ = writeln!(output, "Path: {}", report.path.display());
        match &report.clone_status {
            CloneStatus::Cloned => {
                let _ = writeln!(output, "Status: cloned");
            }
            CloneStatus::Local => {
                let _ = writeln!(output, "Status: local");
            }
            CloneStatus::Failed(error) => {
                let _ = writeln!(output, "Status: failed ({error})");
                let _ = writeln!(output);
                continue;
            }
            CloneStatus::Pending => {
                let _ = writeln!(output, "Status: pending");
                let _ = writeln!(output);
                continue;
            }
        }

        if report.results.is_empty() {
            let _ = writeln!(output, "Refit results: none");
        } else {
            let _ = writeln!(output, "Refit results:");
            for result in &report.results {
                let _ = writeln!(output, "{result}");
            }
        }

        if !report.errors.is_empty() {
            let _ = writeln!(output, "Refit errors:");
            for error in &report.errors {
                let _ = writeln!(output, "- {error}");
            }
        }

        let _ = writeln!(output);
    }
    output
}

fn render_launch_text(reports: &[LaunchReport]) -> String {
    let mut output = String::new();
    for report in reports {
        let _ = writeln!(output, "Source: {}", report.source);
        let _ = writeln!(output, "Path: {}", report.path.display());
        match &report.clone_status {
            CloneStatus::Cloned => {
                let _ = writeln!(output, "Status: cloned");
            }
            CloneStatus::Local => {
                let _ = writeln!(output, "Status: local");
            }
            CloneStatus::Failed(error) => {
                let _ = writeln!(output, "Status: failed ({error})");
                let _ = writeln!(output);
                continue;
            }
            CloneStatus::Pending => {
                let _ = writeln!(output, "Status: pending");
                let _ = writeln!(output);
                continue;
            }
        }

        match &report.dockerfile {
            Some(contents) => {
                let _ = writeln!(output, "Dockerfile:");
                let _ = writeln!(output, "{contents}");
            }
            None => {
                let _ = writeln!(output, "Dockerfile: unavailable");
            }
        }

        match &report.ci_config {
            Some(contents) => {
                let _ = writeln!(output, ".gitlab-ci.yml:");
                let _ = writeln!(output, "{contents}");
            }
            None => {
                let _ = writeln!(output, "CI config: unavailable");
            }
        }

        if !report.errors.is_empty() {
            let _ = writeln!(output, "Launch errors:");
            for error in &report.errors {
                let _ = writeln!(output, "- {error}");
            }
        }

        let _ = writeln!(output);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        BatchSource, CloneStatus, LaunchReport, OutputArgs, OutputFormat, RefitReport, RepoReport,
        RepoSourceArgs, audit_local, clone_and_audit, clone_repo, emit_audit_reports,
        emit_launch_reports, emit_refit_reports, launch_local, load_repo_paths_from_dir,
        load_repo_targets, load_repo_urls, populate_audit, populate_launch, populate_refit,
        refit_local, render_audit_text, render_launch_text, render_refit_text, repo_dir_name,
        resolve_source_args, run_audit, run_launch, run_refit,
    };
    use shipshape_core::{Mechanic, ShipShapeError, Violation, format_language_stats};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;

    #[test]
    fn repo_dir_name_handles_https() {
        let name = repo_dir_name("https://github.com/org/repo.git");
        assert_eq!(name, "repo");
    }

    #[test]
    fn repo_dir_name_handles_ssh() {
        let name = repo_dir_name("git@github.com:org/repo.git");
        assert_eq!(name, "repo");
    }

    #[test]
    fn resolve_source_prefers_file_over_url() {
        let args = RepoSourceArgs {
            file: Some(PathBuf::from("repos.txt")),
            url: Some("https://example.com/repo.git".to_string()),
            dir: None,
            path: None,
        };

        let source = resolve_source_args(&args).expect("source");
        match source {
            BatchSource::File(path) => assert_eq!(path, PathBuf::from("repos.txt")),
            _ => panic!("expected file source"),
        }
    }

    #[test]
    fn resolve_source_trims_url() {
        let args = RepoSourceArgs {
            file: None,
            url: Some(" https://example.com/repo.git ".to_string()),
            dir: None,
            path: None,
        };

        let source = resolve_source_args(&args).expect("source");
        match source {
            BatchSource::Url(url) => assert_eq!(url, "https://example.com/repo.git"),
            _ => panic!("expected url source"),
        }
    }

    #[tokio::test]
    async fn load_repo_paths_from_dir_filters_hidden() {
        let root = std::env::temp_dir().join(unique_dir_name());
        let repo_a = root.join("repo-a");
        let repo_b = root.join("repo-b");
        let hidden = root.join(".hidden");
        std::fs::create_dir_all(&repo_a).expect("repo a");
        std::fs::create_dir_all(&repo_b).expect("repo b");
        std::fs::create_dir_all(&hidden).expect("hidden dir");
        std::fs::write(root.join("notes.txt"), "data").expect("file");

        let mut repos = load_repo_paths_from_dir(&root).await.expect("repos");
        repos.sort();

        assert_eq!(repos, vec![repo_a, repo_b]);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    static UNIQUE_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn unique_dir_name() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let counter = UNIQUE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        PathBuf::from(format!("shipshape_cli_test_{nanos}_{counter}"))
    }

    #[test]
    fn resolve_source_errors_when_missing_or_empty() {
        let empty_url = RepoSourceArgs {
            file: None,
            url: Some("   ".to_string()),
            dir: None,
            path: None,
        };
        assert!(resolve_source_args(&empty_url).is_err());

        let missing = RepoSourceArgs {
            file: None,
            url: None,
            dir: None,
            path: None,
        };
        assert!(resolve_source_args(&missing).is_err());
    }

    #[tokio::test]
    async fn load_repo_urls_ignores_comments_and_blank_lines() {
        let root = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&root).expect("create temp dir");
        let file_path = root.join("repos.txt");
        std::fs::write(
            &file_path,
            "# comment\n\nhttps://example.com/a.git\n  \nhttps://example.com/b.git\n",
        )
        .expect("write repo list");

        let urls = load_repo_urls(&file_path).await.expect("urls");

        assert_eq!(
            urls,
            vec!["https://example.com/a.git", "https://example.com/b.git"]
        );

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[tokio::test]
    async fn load_repo_targets_supports_all_sources() {
        let root = std::env::temp_dir().join(unique_dir_name());
        let batch_dir = root.join("batch");
        std::fs::create_dir_all(&batch_dir).expect("create dir");
        let file_path = root.join("repos.txt");
        std::fs::write(&file_path, "https://example.com/a.git\n").expect("write file");
        let repo_dir = batch_dir.join("repo-a");
        std::fs::create_dir_all(&repo_dir).expect("repo dir");

        let output = root.join("out");

        let file_targets = load_repo_targets(BatchSource::File(file_path), &output)
            .await
            .expect("file targets");
        assert_eq!(file_targets.len(), 1);

        let url_targets = load_repo_targets(
            BatchSource::Url("https://example.com/b.git".to_string()),
            &output,
        )
        .await
        .expect("url targets");
        assert_eq!(url_targets.len(), 1);

        let dir_targets = load_repo_targets(BatchSource::Dir(batch_dir.clone()), &output)
            .await
            .expect("dir targets");
        assert_eq!(dir_targets.len(), 1);

        let path_targets = load_repo_targets(BatchSource::Path(repo_dir.clone()), &output)
            .await
            .expect("path targets");
        assert_eq!(path_targets.len(), 1);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[tokio::test]
    async fn audit_refit_launch_local_fail_when_missing() {
        let missing = std::env::temp_dir().join(unique_dir_name());
        let mechanics = Arc::new(Vec::new());

        let audit_report = audit_local(missing.clone(), mechanics.clone()).await;
        assert!(matches!(audit_report.clone_status, CloneStatus::Failed(_)));

        let refit_report = refit_local(missing.clone(), mechanics.clone(), false).await;
        assert!(matches!(refit_report.clone_status, CloneStatus::Failed(_)));

        let launch_report = launch_local(missing.clone()).await;
        assert!(matches!(launch_report.clone_status, CloneStatus::Failed(_)));
    }

    #[tokio::test]
    async fn clone_repo_handles_success_and_failure() {
        let source = init_git_repo();
        let dest = std::env::temp_dir().join(unique_dir_name());

        clone_repo(source.to_str().unwrap(), &dest)
            .await
            .expect("clone succeeds");

        let bad_dest = std::env::temp_dir().join(unique_dir_name());
        let missing_repo = std::env::temp_dir().join(unique_dir_name());
        let result = clone_repo(missing_repo.to_str().unwrap(), &bad_dest).await;
        assert!(result.is_err());

        std::fs::remove_dir_all(&source).expect("cleanup source");
        std::fs::remove_dir_all(&dest).expect("cleanup dest");
    }

    #[tokio::test]
    async fn clone_and_audit_handles_existing_destination() {
        let source = "https://example.com/repo.git".to_string();
        let dest = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&dest).expect("create dest");

        let mechanics = Arc::new(Vec::new());
        let report = clone_and_audit(source, dest.clone(), mechanics).await;

        assert!(matches!(report.clone_status, CloneStatus::Failed(_)));

        std::fs::remove_dir_all(&dest).expect("cleanup dest");
    }

    #[test]
    fn populate_audit_collects_language_stats_and_violations() {
        let repo = temp_repo_with_file("src/main.rs", "fn main() {}\n");
        let mut report = RepoReport::new("local".to_string(), repo.clone());
        let mechanics: Vec<Arc<dyn Mechanic + Send + Sync>> = vec![Arc::new(TestMechanic {
            id: "demo",
            audit_result: Ok(vec![Violation {
                id: "demo".to_string(),
                message: "violation".to_string(),
            }]),
            dry_run_result: Ok(String::new()),
            apply_result: Ok(false),
        })];

        populate_audit(&mut report, &mechanics);

        assert!(report.language_stats.is_some());
        assert_eq!(report.violations.len(), 1);

        std::fs::remove_dir_all(&repo).expect("cleanup repo");
    }

    #[test]
    fn populate_audit_tracks_language_errors() {
        let mut report = RepoReport::new(
            "missing".to_string(),
            std::env::temp_dir().join(unique_dir_name()),
        );
        let mechanics = Vec::new();

        populate_audit(&mut report, &mechanics);

        assert!(!report.audit_errors.is_empty());
    }

    #[test]
    fn populate_refit_handles_apply_and_dry_run_paths() {
        let repo = temp_repo_with_file("main.rs", "fn main() {}\n");
        let mut report = RefitReport::new("local".to_string(), repo.clone());
        let mechanics: Vec<Arc<dyn Mechanic + Send + Sync>> = vec![
            Arc::new(TestMechanic {
                id: "apply",
                audit_result: Ok(Vec::new()),
                dry_run_result: Ok(String::new()),
                apply_result: Ok(true),
            }),
            Arc::new(TestMechanic {
                id: "dry",
                audit_result: Ok(Vec::new()),
                dry_run_result: Ok("diff".to_string()),
                apply_result: Ok(false),
            }),
            Arc::new(TestMechanic {
                id: "err",
                audit_result: Ok(Vec::new()),
                dry_run_result: Err("bad".to_string()),
                apply_result: Err("apply bad".to_string()),
            }),
        ];

        populate_refit(&mut report, &mechanics, true);
        assert!(report.results.iter().any(|line| line.contains("apply")));
        assert!(report.errors.iter().any(|line| line.contains("apply bad")));

        let mut report = RefitReport::new("local".to_string(), repo.clone());
        populate_refit(&mut report, &mechanics, false);
        assert!(report.results.iter().any(|line| line.contains("dry-run")));
        assert!(report.errors.iter().any(|line| line.contains("bad")));

        std::fs::remove_dir_all(&repo).expect("cleanup repo");
    }

    #[test]
    fn populate_launch_sets_outputs() {
        let repo = temp_repo_with_file("Cargo.toml", "[package]\nname = \"demo\"\n");
        let mut report = LaunchReport::new("local".to_string(), repo.clone());

        populate_launch(&mut report);

        assert!(report.dockerfile.is_some());
        assert!(report.ci_config.is_some());

        std::fs::remove_dir_all(&repo).expect("cleanup repo");
    }

    #[test]
    fn format_language_stats_sorts_descending() {
        let mut stats = BTreeMap::new();
        stats.insert("Rust".to_string(), 40.0);
        stats.insert("Python".to_string(), 60.0);
        let sorted = format_language_stats(&stats);

        assert_eq!(sorted[0].0, "Python");
        assert_eq!(sorted[1].0, "Rust");
    }

    #[test]
    fn render_audit_text_covers_branches() {
        let mut report_a = RepoReport::new("cloned".to_string(), PathBuf::from("/tmp/a"));
        report_a.clone_status = CloneStatus::Cloned;
        let mut stats = BTreeMap::new();
        stats.insert("Rust".to_string(), 80.0);
        report_a.language_stats = Some(stats);
        report_a.violations = vec![Violation {
            id: "v1".to_string(),
            message: "m1".to_string(),
        }];

        let mut report_b = RepoReport::new("local".to_string(), PathBuf::from("/tmp/b"));
        report_b.clone_status = CloneStatus::Local;
        report_b.language_stats = Some(BTreeMap::new());
        report_b.audit_errors = vec!["err".to_string()];

        let mut report_c = RepoReport::new("none".to_string(), PathBuf::from("/tmp/c"));
        report_c.clone_status = CloneStatus::Cloned;
        report_c.language_stats = None;

        let mut report_failed = RepoReport::new("failed".to_string(), PathBuf::from("/tmp/d"));
        report_failed.clone_status = CloneStatus::Failed("oops".to_string());

        let mut report_pending = RepoReport::new("pending".to_string(), PathBuf::from("/tmp/e"));
        report_pending.clone_status = CloneStatus::Pending;

        let output =
            render_audit_text(&[report_a, report_b, report_c, report_failed, report_pending]);

        assert!(output.contains("Status: cloned"));
        assert!(output.contains("Status: local"));
        assert!(output.contains("Status: failed (oops)"));
        assert!(output.contains("Status: pending"));
        assert!(output.contains("Languages: none detected"));
        assert!(output.contains("Languages: unavailable"));
        assert!(output.contains("Violations: none"));
        assert!(output.contains("[v1] m1"));
        assert!(output.contains("Audit errors:"));
    }

    #[test]
    fn render_refit_text_covers_branches() {
        let mut report_a = RefitReport::new("cloned".to_string(), PathBuf::from("/tmp/a"));
        report_a.clone_status = CloneStatus::Cloned;
        report_a.results = vec!["apply: ok".to_string()];

        let mut report_b = RefitReport::new("local".to_string(), PathBuf::from("/tmp/b"));
        report_b.clone_status = CloneStatus::Local;
        report_b.errors = vec!["err".to_string()];

        let mut report_failed = RefitReport::new("failed".to_string(), PathBuf::from("/tmp/c"));
        report_failed.clone_status = CloneStatus::Failed("oops".to_string());

        let mut report_pending = RefitReport::new("pending".to_string(), PathBuf::from("/tmp/d"));
        report_pending.clone_status = CloneStatus::Pending;

        let output = render_refit_text(&[report_a, report_b, report_failed, report_pending]);

        assert!(output.contains("Refit results:"));
        assert!(output.contains("apply: ok"));
        assert!(output.contains("Refit results: none"));
        assert!(output.contains("Refit errors:"));
        assert!(output.contains("Status: failed (oops)"));
        assert!(output.contains("Status: pending"));
    }

    #[test]
    fn render_launch_text_covers_branches() {
        let mut report_a = LaunchReport::new("cloned".to_string(), PathBuf::from("/tmp/a"));
        report_a.clone_status = CloneStatus::Cloned;
        report_a.dockerfile = Some("FROM rust".to_string());
        report_a.ci_config = Some("stages:".to_string());

        let mut report_b = LaunchReport::new("local".to_string(), PathBuf::from("/tmp/b"));
        report_b.clone_status = CloneStatus::Local;
        report_b.errors = vec!["err".to_string()];

        let mut report_failed = LaunchReport::new("failed".to_string(), PathBuf::from("/tmp/c"));
        report_failed.clone_status = CloneStatus::Failed("oops".to_string());

        let mut report_pending = LaunchReport::new("pending".to_string(), PathBuf::from("/tmp/d"));
        report_pending.clone_status = CloneStatus::Pending;

        let output = render_launch_text(&[report_a, report_b, report_failed, report_pending]);

        assert!(output.contains("Dockerfile:"));
        assert!(output.contains("FROM rust"));
        assert!(output.contains(".gitlab-ci.yml:"));
        assert!(output.contains("CI config: unavailable"));
        assert!(output.contains("Launch errors:"));
        assert!(output.contains("Status: failed (oops)"));
        assert!(output.contains("Status: pending"));
    }

    #[tokio::test]
    async fn emit_reports_support_formats() {
        let root = std::env::temp_dir().join(unique_dir_name());

        let audit_path = root.join("out/audit.md");
        let output = OutputArgs {
            format: OutputFormat::Markdown,
            report_output: Some(audit_path.clone()),
        };
        let report = RepoReport::new("repo".to_string(), PathBuf::from("/tmp/repo"));
        emit_audit_reports(&[report], &output)
            .await
            .expect("emit markdown");
        let contents = std::fs::read_to_string(&audit_path).expect("read markdown");
        assert!(contents.contains("# ShipShape Audit Report"));

        let json_path = root.join("out/refit.json");
        let output = OutputArgs {
            format: OutputFormat::Json,
            report_output: Some(json_path.clone()),
        };
        let report = RefitReport::new("repo".to_string(), PathBuf::from("/tmp/repo"));
        emit_refit_reports(&[report], &output)
            .await
            .expect("emit json");
        let contents = std::fs::read_to_string(&json_path).expect("read json");
        assert!(contents.contains("\"cloneStatus\""));

        let output = OutputArgs {
            format: OutputFormat::Text,
            report_output: None,
        };
        let report = LaunchReport::new("repo".to_string(), PathBuf::from("/tmp/repo"));
        emit_launch_reports(&[report], &output)
            .await
            .expect("emit text");

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[tokio::test]
    async fn run_flows_handle_empty_and_local_targets() {
        let root = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&root).expect("create root");
        let output = root.join("out");
        let report = OutputArgs {
            format: OutputFormat::Text,
            report_output: None,
        };

        run_audit(
            BatchSource::Dir(root.clone()),
            output.clone(),
            1,
            Vec::new(),
            report.clone(),
        )
        .await
        .expect("audit empty");

        let repo = temp_repo_with_file("src/lib.rs", "pub fn demo() {}\n");

        run_audit(
            BatchSource::Path(repo.clone()),
            output.clone(),
            1,
            Vec::new(),
            report.clone(),
        )
        .await
        .expect("audit local");

        run_refit(
            BatchSource::Path(repo.clone()),
            output.clone(),
            0,
            vec!["noop".to_string()],
            report.clone(),
            false,
        )
        .await
        .expect("refit local");

        run_launch(
            BatchSource::Path(repo.clone()),
            output.clone(),
            1,
            report.clone(),
        )
        .await
        .expect("launch local");

        std::fs::remove_dir_all(&repo).expect("cleanup repo");
        std::fs::remove_dir_all(&root).expect("cleanup root");
    }

    fn temp_repo_with_file(rel_path: &str, contents: &str) -> PathBuf {
        let root = std::env::temp_dir().join(unique_dir_name());
        let file_path = root.join(rel_path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create dir");
        }
        std::fs::write(&file_path, contents).expect("write file");
        root
    }

    fn init_git_repo() -> PathBuf {
        let root = std::env::temp_dir().join(unique_dir_name());
        std::fs::create_dir_all(&root).expect("create repo");
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .expect("git init");
        std::fs::write(root.join("README.md"), "shipshape").expect("write readme");
        Command::new("git")
            .args(["add", "."])
            .current_dir(&root)
            .status()
            .expect("git add");
        Command::new("git")
            .args([
                "-c",
                "user.name=ShipShape",
                "-c",
                "user.email=shipshape@example.com",
                "commit",
                "-m",
                "init",
            ])
            .current_dir(&root)
            .status()
            .expect("git commit");
        root
    }

    struct TestMechanic {
        id: &'static str,
        audit_result: Result<Vec<Violation>, String>,
        dry_run_result: Result<String, String>,
        apply_result: Result<bool, String>,
    }

    impl shipshape_core::Mechanic for TestMechanic {
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
