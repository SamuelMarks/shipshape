//! GitHub PR + GitLab mirror workflow orchestration.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use chrono::Utc;
use diesel::prelude::*;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use shipshape_core::{FleetReport, PrTemplateContext, StdFileSystem, interpolate_pr_template};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::DbPool;
use crate::models::{NewWorkflow, NewWorkflowStep};
use crate::schema::{workflow_steps, workflows};

/// Repository metadata needed for workflow orchestration.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RepoSpec {
    /// GitHub repository URL.
    pub repo_url: String,
    /// Base branch for the pull request.
    pub base_branch: String,
    /// Optional local checkout to use instead of cloning.
    pub local_path: Option<String>,
}

/// Patch data to apply before opening a pull request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PatchSpec {
    /// Unified diff content to apply.
    pub diff: String,
    /// Feature branch name to create.
    pub branch: String,
    /// Commit message for the applied patch.
    pub commit_message: String,
}

/// GitHub pull request metadata.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PullRequestSpec {
    /// Pull request title.
    pub title: String,
    /// Optional pull request body.
    pub body: Option<String>,
    /// Whether the PR should be opened as a draft.
    #[serde(default)]
    pub draft: bool,
}

/// GitLab mirror and pipeline metadata.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GitLabSpec {
    /// GitLab mirror URL.
    pub mirror_url: String,
    /// GitLab project path (namespace/project).
    pub project_path: String,
    /// Git reference to trigger (defaults to the feature branch if omitted).
    pub pipeline_ref: Option<String>,
}

/// Request payload for the GitHub PR + GitLab mirror workflow.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowRequest {
    /// GitHub repository metadata.
    pub repo: RepoSpec,
    /// Patch/branch metadata.
    pub patch: PatchSpec,
    /// Pull request metadata.
    pub pr: PullRequestSpec,
    /// Fleet health report used for PR template interpolation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_report: Option<FleetReport>,
    /// GitLab mirror metadata.
    pub gitlab: GitLabSpec,
}

/// Workflow status values.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    /// Workflow step completed successfully.
    Success,
    /// Workflow step failed.
    Failed,
    /// Workflow step skipped due to earlier failure.
    Skipped,
}

impl WorkflowStatus {
    /// Human-readable status label.
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowStatus::Success => "success",
            WorkflowStatus::Failed => "failed",
            WorkflowStatus::Skipped => "skipped",
        }
    }
}

/// Workflow step identifiers.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepKind {
    /// Apply the patch diff.
    ApplyPatch,
    /// Create a feature branch.
    CreateBranch,
    /// Push the branch to GitHub.
    PushBranch,
    /// Ensure the GitLab project exists for mirroring.
    EnsureGitlabProject,
    /// Push the branch to the GitLab mirror.
    MirrorPush,
    /// Trigger GitLab CI pipeline.
    TriggerGitlab,
    /// Open the GitHub pull request.
    OpenPr,
}

impl WorkflowStepKind {
    /// Human-readable step label.
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowStepKind::ApplyPatch => "apply_patch",
            WorkflowStepKind::CreateBranch => "create_branch",
            WorkflowStepKind::PushBranch => "push_branch",
            WorkflowStepKind::EnsureGitlabProject => "ensure_gitlab_project",
            WorkflowStepKind::MirrorPush => "mirror_push",
            WorkflowStepKind::TriggerGitlab => "trigger_gitlab",
            WorkflowStepKind::OpenPr => "open_pr",
        }
    }
}

/// Captures the outcome of a single workflow step.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowStep {
    /// Step identifier.
    pub kind: WorkflowStepKind,
    /// Step status.
    pub status: WorkflowStatus,
    /// Optional detail or error message.
    pub detail: Option<String>,
}

/// Response payload for the GitHub PR + GitLab mirror workflow.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowResult {
    /// Workflow identifier.
    pub workflow_id: String,
    /// Overall workflow status.
    pub status: WorkflowStatus,
    /// Step-by-step results.
    pub steps: Vec<WorkflowStep>,
    /// GitHub pull request URL if created.
    pub pr_url: Option<String>,
    /// GitLab pipeline URL if triggered.
    pub pipeline_url: Option<String>,
}

/// Error type for workflow orchestration.
#[derive(Debug, Clone)]
pub struct WorkflowError {
    message: String,
}

impl WorkflowError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for WorkflowError {}

/// Repository workspace used during git operations.
#[derive(Debug, Clone)]
pub struct GitWorkspace {
    path: PathBuf,
    managed: bool,
}

/// Trait for git operations.
pub trait GitClient {
    /// Prepare a workspace for the repository.
    fn prepare_workspace(&self, repo: &RepoSpec) -> Result<GitWorkspace, WorkflowError>;
    /// Apply a patch to the repository.
    fn apply_patch(
        &self,
        workspace: &GitWorkspace,
        patch: &PatchSpec,
    ) -> Result<String, WorkflowError>;
    /// Create a new branch for the patch.
    fn create_branch(
        &self,
        workspace: &GitWorkspace,
        base_branch: &str,
        branch: &str,
    ) -> Result<(), WorkflowError>;
    /// Push the branch to the primary remote.
    fn push_branch(
        &self,
        workspace: &GitWorkspace,
        repo: &RepoSpec,
        branch: &str,
    ) -> Result<String, WorkflowError>;
    /// Push the branch to the GitLab mirror remote.
    fn push_mirror(
        &self,
        workspace: &GitWorkspace,
        gitlab: &GitLabSpec,
        branch: &str,
    ) -> Result<String, WorkflowError>;
    /// Clean up any managed workspace resources.
    fn cleanup(&self, workspace: GitWorkspace) -> Result<(), WorkflowError>;
}

/// Trait for GitHub API operations.
pub trait GitHubClient {
    /// Open a pull request for the branch.
    fn open_pr(
        &self,
        repo: &RepoSpec,
        pr: &PullRequestSpec,
        branch: &str,
    ) -> Result<String, WorkflowError>;
}

/// Trait for GitLab API operations.
pub trait GitLabClient {
    /// Ensure the GitLab project exists for mirroring.
    fn ensure_project(&self, gitlab: &GitLabSpec) -> Result<(), WorkflowError>;
    /// Trigger a pipeline for the mirror.
    fn trigger_pipeline(&self, gitlab: &GitLabSpec, branch: &str) -> Result<String, WorkflowError>;
}

/// Mock git client used for local testing.
#[derive(Debug, Default, Clone)]
pub struct MockGitClient;

impl GitClient for MockGitClient {
    fn prepare_workspace(&self, repo: &RepoSpec) -> Result<GitWorkspace, WorkflowError> {
        if repo.repo_url.trim().is_empty() {
            return Err(WorkflowError::new("repo_url is required"));
        }
        if repo.base_branch.trim().is_empty() {
            return Err(WorkflowError::new("base_branch is required"));
        }
        Ok(GitWorkspace {
            path: PathBuf::from("/tmp/shipshape-mock"),
            managed: false,
        })
    }

    fn apply_patch(
        &self,
        _workspace: &GitWorkspace,
        patch: &PatchSpec,
    ) -> Result<String, WorkflowError> {
        if patch.diff.trim().is_empty() {
            return Err(WorkflowError::new("patch diff is empty"));
        }
        Ok("mock-commit-sha".to_string())
    }

    fn create_branch(
        &self,
        _workspace: &GitWorkspace,
        base_branch: &str,
        branch: &str,
    ) -> Result<(), WorkflowError> {
        if base_branch.trim().is_empty() {
            return Err(WorkflowError::new("base branch is required"));
        }
        if branch.trim().is_empty() {
            return Err(WorkflowError::new("branch name is required"));
        }
        Ok(())
    }

    fn push_branch(
        &self,
        _workspace: &GitWorkspace,
        repo: &RepoSpec,
        branch: &str,
    ) -> Result<String, WorkflowError> {
        if repo.repo_url.trim().is_empty() {
            return Err(WorkflowError::new("repo_url is required"));
        }
        Ok(format!("origin/{branch}"))
    }

    fn push_mirror(
        &self,
        _workspace: &GitWorkspace,
        gitlab: &GitLabSpec,
        branch: &str,
    ) -> Result<String, WorkflowError> {
        if gitlab.mirror_url.trim().is_empty() {
            return Err(WorkflowError::new("gitlab mirror_url is required"));
        }
        Ok(format!("mirror/{branch}"))
    }

    fn cleanup(&self, _workspace: GitWorkspace) -> Result<(), WorkflowError> {
        Ok(())
    }
}

/// Mock GitHub client used for local testing.
#[derive(Debug, Default, Clone)]
pub struct MockGitHubClient;

impl GitHubClient for MockGitHubClient {
    fn open_pr(
        &self,
        repo: &RepoSpec,
        pr: &PullRequestSpec,
        _branch: &str,
    ) -> Result<String, WorkflowError> {
        if repo.repo_url.trim().is_empty() {
            return Err(WorkflowError::new("repo_url is required"));
        }
        if pr.title.trim().is_empty() {
            return Err(WorkflowError::new("pull request title is required"));
        }
        Ok("https://github.com/shipshape/mock/pull/1".to_string())
    }
}

/// Mock GitLab client used for local testing.
#[derive(Debug, Default, Clone)]
pub struct MockGitLabClient;

impl GitLabClient for MockGitLabClient {
    fn ensure_project(&self, gitlab: &GitLabSpec) -> Result<(), WorkflowError> {
        if gitlab.project_path.trim().is_empty() {
            return Err(WorkflowError::new("gitlab project_path is required"));
        }
        Ok(())
    }

    fn trigger_pipeline(&self, gitlab: &GitLabSpec, branch: &str) -> Result<String, WorkflowError> {
        if gitlab.project_path.trim().is_empty() {
            return Err(WorkflowError::new("gitlab project_path is required"));
        }
        let reference = gitlab
            .pipeline_ref
            .as_ref()
            .map(String::as_str)
            .unwrap_or(branch);
        Ok(format!(
            "https://gitlab.example.com/{}/-/pipelines/{}",
            gitlab.project_path, reference
        ))
    }
}

/// Git client implemented with `git` shell commands.
#[derive(Debug, Clone)]
pub struct GitCommandClient {
    workspace_root: PathBuf,
    keep_workspace: bool,
    github_token: Option<String>,
    gitlab_token: Option<String>,
    author_name: String,
    author_email: String,
}

impl GitCommandClient {
    /// Build a git command client from environment variables.
    pub fn from_env() -> Self {
        let workspace_root = std::env::var("SHIPSHAPE_WORKSPACE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("shipshape-workflows"));
        let keep_workspace = std::env::var("SHIPSHAPE_KEEP_WORKSPACE")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let github_token = std::env::var("GITHUB_TOKEN").ok();
        let gitlab_token = std::env::var("GITLAB_TOKEN").ok();
        let author_name = std::env::var("SHIPSHAPE_GIT_AUTHOR_NAME")
            .unwrap_or_else(|_| "ShipShape Bot".to_string());
        let author_email = std::env::var("SHIPSHAPE_GIT_AUTHOR_EMAIL")
            .unwrap_or_else(|_| "shipshape@example.com".to_string());
        Self {
            workspace_root,
            keep_workspace,
            github_token,
            gitlab_token,
            author_name,
            author_email,
        }
    }
}

impl GitClient for GitCommandClient {
    fn prepare_workspace(&self, repo: &RepoSpec) -> Result<GitWorkspace, WorkflowError> {
        if repo.base_branch.trim().is_empty() {
            return Err(WorkflowError::new("base_branch is required"));
        }
        if let Some(local_path) = repo.local_path.as_ref() {
            let path = PathBuf::from(local_path);
            if !path.is_dir() {
                return Err(WorkflowError::new(format!(
                    "local_path not found: {local_path}"
                )));
            }
            if !path.join(".git").is_dir() {
                return Err(WorkflowError::new("local_path is not a git repository"));
            }
            return Ok(GitWorkspace {
                path,
                managed: false,
            });
        }

        if repo.repo_url.trim().is_empty() {
            return Err(WorkflowError::new("repo_url is required"));
        }

        std::fs::create_dir_all(&self.workspace_root)
            .map_err(|err| WorkflowError::new(format!("create workspace root failed: {err}")))?;
        let workspace_path = self.workspace_root.join(Uuid::new_v4().to_string());
        let clone_url = maybe_inject_token(
            &repo.repo_url,
            self.github_token.as_deref(),
            "x-access-token",
        );
        run_git(
            &self.workspace_root,
            &[
                "clone",
                &clone_url,
                workspace_path.to_str().unwrap_or_default(),
            ],
            &[],
        )?;
        run_git(
            &workspace_path,
            &["fetch", "origin", &repo.base_branch],
            &[],
        )?;
        run_git(&workspace_path, &["checkout", &repo.base_branch], &[])?;

        Ok(GitWorkspace {
            path: workspace_path,
            managed: true,
        })
    }

    fn apply_patch(
        &self,
        workspace: &GitWorkspace,
        patch: &PatchSpec,
    ) -> Result<String, WorkflowError> {
        if patch.diff.trim().is_empty() {
            return Err(WorkflowError::new("patch diff is empty"));
        }

        let patch_path = workspace.path.join("shipshape.patch");
        std::fs::write(&patch_path, patch.diff.as_bytes())
            .map_err(|err| WorkflowError::new(format!("write patch failed: {err}")))?;
        run_git(
            &workspace.path,
            &["apply", patch_path.to_str().unwrap_or_default()],
            &[],
        )?;

        let status = run_git(&workspace.path, &["status", "--porcelain"], &[])?;
        if status.trim().is_empty() {
            return Err(WorkflowError::new("patch produced no changes"));
        }

        run_git(&workspace.path, &["add", "-A"], &[])?;
        let envs = [
            ("GIT_AUTHOR_NAME", self.author_name.as_str()),
            ("GIT_AUTHOR_EMAIL", self.author_email.as_str()),
            ("GIT_COMMITTER_NAME", self.author_name.as_str()),
            ("GIT_COMMITTER_EMAIL", self.author_email.as_str()),
        ];
        run_git(
            &workspace.path,
            &["commit", "-m", patch.commit_message.as_str()],
            &envs,
        )?;

        let commit = run_git(&workspace.path, &["rev-parse", "HEAD"], &[])?;
        Ok(commit.trim().to_string())
    }

    fn create_branch(
        &self,
        workspace: &GitWorkspace,
        base_branch: &str,
        branch: &str,
    ) -> Result<(), WorkflowError> {
        if branch.trim().is_empty() {
            return Err(WorkflowError::new("branch name is required"));
        }
        run_git(&workspace.path, &["checkout", base_branch], &[])?;
        run_git(
            &workspace.path,
            &["checkout", "-B", branch, base_branch],
            &[],
        )?;
        Ok(())
    }

    fn push_branch(
        &self,
        workspace: &GitWorkspace,
        repo: &RepoSpec,
        branch: &str,
    ) -> Result<String, WorkflowError> {
        let remote_url = maybe_inject_token(
            &repo.repo_url,
            self.github_token.as_deref(),
            "x-access-token",
        );
        run_git(
            &workspace.path,
            &["remote", "set-url", "origin", remote_url.as_str()],
            &[],
        )?;
        run_git(&workspace.path, &["push", "origin", branch], &[])?;
        Ok(format!("origin/{branch}"))
    }

    fn push_mirror(
        &self,
        workspace: &GitWorkspace,
        gitlab: &GitLabSpec,
        branch: &str,
    ) -> Result<String, WorkflowError> {
        if gitlab.mirror_url.trim().is_empty() {
            return Err(WorkflowError::new("gitlab mirror_url is required"));
        }
        let mirror_url =
            maybe_inject_token(&gitlab.mirror_url, self.gitlab_token.as_deref(), "oauth2");
        run_git(
            &workspace.path,
            &["remote", "set-url", "shipshape-mirror", mirror_url.as_str()],
            &[],
        )
        .or_else(|_| {
            run_git(
                &workspace.path,
                &["remote", "add", "shipshape-mirror", mirror_url.as_str()],
                &[],
            )
        })?;
        run_git(&workspace.path, &["push", "shipshape-mirror", branch], &[])?;
        Ok(format!("mirror/{branch}"))
    }

    fn cleanup(&self, workspace: GitWorkspace) -> Result<(), WorkflowError> {
        if workspace.managed && !self.keep_workspace {
            std::fs::remove_dir_all(&workspace.path)
                .map_err(|err| WorkflowError::new(format!("cleanup workspace failed: {err}")))?;
        }
        Ok(())
    }
}

/// GitHub API client implementation.
#[derive(Debug, Clone)]
pub struct GitHubApiClient {
    base_url: String,
    token: Option<String>,
    client: Client,
    user_agent: String,
}

impl GitHubApiClient {
    /// Build a GitHub API client from environment variables.
    pub fn from_env() -> Self {
        let base_url = std::env::var("GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_string());
        let token = std::env::var("GITHUB_TOKEN").ok();
        let user_agent =
            std::env::var("GITHUB_USER_AGENT").unwrap_or_else(|_| "shipshape-server".to_string());
        Self {
            base_url,
            token,
            client: Client::new(),
            user_agent,
        }
    }
}

impl GitHubClient for GitHubApiClient {
    fn open_pr(
        &self,
        repo: &RepoSpec,
        pr: &PullRequestSpec,
        branch: &str,
    ) -> Result<String, WorkflowError> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| WorkflowError::new("GITHUB_TOKEN is required"))?;
        let (owner, name) = parse_github_repo(&repo.repo_url)?;
        let url = format!(
            "{}/repos/{owner}/{name}/pulls",
            self.base_url.trim_end_matches('/')
        );
        let body = serde_json::json!({
            "title": pr.title,
            "head": branch,
            "base": repo.base_branch,
            "body": pr.body,
            "draft": pr.draft,
        });
        let response = self
            .client
            .post(url)
            .header("User-Agent", &self.user_agent)
            .bearer_auth(token)
            .json(&body)
            .send()
            .map_err(|err| WorkflowError::new(format!("github request failed: {err}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(WorkflowError::new(format!(
                "github api error ({status}): {body}"
            )));
        }
        let value: serde_json::Value = response
            .json()
            .map_err(|err| WorkflowError::new(format!("github response decode failed: {err}")))?;
        let html_url = value
            .get("html_url")
            .and_then(|val| val.as_str())
            .ok_or_else(|| WorkflowError::new("github response missing html_url"))?;
        Ok(html_url.to_string())
    }
}

/// GitLab API client implementation.
#[derive(Debug, Clone)]
pub struct GitLabApiClient {
    base_url: String,
    token: Option<String>,
    client: Client,
}

#[derive(Debug, Deserialize)]
struct GitLabNamespace {
    id: i64,
    full_path: String,
}

impl GitLabApiClient {
    /// Build a GitLab API client from environment variables.
    pub fn from_env() -> Self {
        let base_url =
            std::env::var("GITLAB_API_URL").unwrap_or_else(|_| "https://gitlab.com".to_string());
        let token = std::env::var("GITLAB_TOKEN").ok();
        Self {
            base_url,
            token,
            client: Client::new(),
        }
    }

    fn resolve_namespace_id(
        &self,
        token: &str,
        namespace_path: &str,
    ) -> Result<i64, WorkflowError> {
        let url = format!("{}/api/v4/namespaces", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .get(url)
            .header("PRIVATE-TOKEN", token)
            .query(&[("search", namespace_path)])
            .send()
            .map_err(|err| WorkflowError::new(format!("gitlab request failed: {err}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(WorkflowError::new(format!(
                "gitlab api error ({status}): {body}"
            )));
        }
        let namespaces: Vec<GitLabNamespace> = response
            .json()
            .map_err(|err| WorkflowError::new(format!("gitlab response decode failed: {err}")))?;
        let namespace = namespaces
            .into_iter()
            .find(|candidate| candidate.full_path == namespace_path)
            .ok_or_else(|| {
                WorkflowError::new(format!("gitlab namespace not found: {namespace_path}"))
            })?;
        Ok(namespace.id)
    }
}

impl GitLabClient for GitLabApiClient {
    fn ensure_project(&self, gitlab: &GitLabSpec) -> Result<(), WorkflowError> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| WorkflowError::new("GITLAB_TOKEN is required"))?;
        if gitlab.project_path.trim().is_empty() {
            return Err(WorkflowError::new("gitlab project_path is required"));
        }
        let base = self.base_url.trim_end_matches('/');
        let encoded = urlencoding::encode(gitlab.project_path.trim());
        let url = format!("{base}/api/v4/projects/{encoded}");
        let response = self
            .client
            .get(url)
            .header("PRIVATE-TOKEN", token)
            .send()
            .map_err(|err| WorkflowError::new(format!("gitlab request failed: {err}")))?;
        if response.status().is_success() {
            return Ok(());
        }
        if response.status().as_u16() != 404 {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(WorkflowError::new(format!(
                "gitlab api error ({status}): {body}"
            )));
        }

        let (namespace_path, project_name) = split_gitlab_project_path(&gitlab.project_path)?;
        let mut form = vec![
            ("name", project_name.clone()),
            ("path", project_name.clone()),
        ];
        if let Some(namespace_path) = namespace_path {
            let namespace_id = self.resolve_namespace_id(token, &namespace_path)?;
            form.push(("namespace_id", namespace_id.to_string()));
        }

        let url = format!("{base}/api/v4/projects");
        let response = self
            .client
            .post(url)
            .header("PRIVATE-TOKEN", token)
            .form(&form)
            .send()
            .map_err(|err| WorkflowError::new(format!("gitlab request failed: {err}")))?;
        if response.status().is_success() || response.status().as_u16() == 409 {
            return Ok(());
        }
        let status = response.status();
        let body = response.text().unwrap_or_default();
        Err(WorkflowError::new(format!(
            "gitlab api error ({status}): {body}"
        )))
    }

    fn trigger_pipeline(&self, gitlab: &GitLabSpec, branch: &str) -> Result<String, WorkflowError> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| WorkflowError::new("GITLAB_TOKEN is required"))?;
        if gitlab.project_path.trim().is_empty() {
            return Err(WorkflowError::new("gitlab project_path is required"));
        }
        let reference = gitlab
            .pipeline_ref
            .as_ref()
            .map(String::as_str)
            .unwrap_or(branch);
        let encoded = urlencoding::encode(&gitlab.project_path);
        let url = format!(
            "{}/api/v4/projects/{}/pipeline",
            self.base_url.trim_end_matches('/'),
            encoded
        );
        let response = self
            .client
            .post(url)
            .header("PRIVATE-TOKEN", token)
            .form(&[("ref", reference)])
            .send()
            .map_err(|err| WorkflowError::new(format!("gitlab request failed: {err}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(WorkflowError::new(format!(
                "gitlab api error ({status}): {body}"
            )));
        }
        let value: serde_json::Value = response
            .json()
            .map_err(|err| WorkflowError::new(format!("gitlab response decode failed: {err}")))?;
        let pipeline_url = value
            .get("web_url")
            .and_then(|val| val.as_str())
            .unwrap_or("");
        if pipeline_url.is_empty() {
            let id = value.get("id").and_then(|val| val.as_i64());
            if let Some(id) = id {
                return Ok(format!(
                    "{}/{}/-/pipelines/{id}",
                    self.base_url.trim_end_matches('/'),
                    gitlab.project_path
                ));
            }
            return Err(WorkflowError::new("gitlab response missing web_url"));
        }
        Ok(pipeline_url.to_string())
    }
}

/// Shared workflow client set.
#[derive(Clone)]
pub struct WorkflowClients {
    git: Arc<dyn GitClient + Send + Sync>,
    github: Arc<dyn GitHubClient + Send + Sync>,
    gitlab: Arc<dyn GitLabClient + Send + Sync>,
}

impl WorkflowClients {
    /// Build mock workflow clients.
    pub fn mock() -> Self {
        Self {
            git: Arc::new(MockGitClient),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(MockGitLabClient),
        }
    }

    /// Build live workflow clients from environment configuration.
    pub fn from_env() -> Self {
        Self {
            git: Arc::new(GitCommandClient::from_env()),
            github: Arc::new(GitHubApiClient::from_env()),
            gitlab: Arc::new(GitLabApiClient::from_env()),
        }
    }
}

/// Workflow orchestration service.
#[derive(Clone)]
pub struct WorkflowService {
    clients: WorkflowClients,
}

impl WorkflowService {
    /// Build a workflow service with mock clients.
    pub fn mock() -> Self {
        Self {
            clients: WorkflowClients::mock(),
        }
    }

    /// Build a workflow service from environment configuration.
    pub fn from_env() -> Self {
        let mode = std::env::var("SHIPSHAPE_WORKFLOW_MODE").unwrap_or_else(|_| "live".to_string());
        if mode.eq_ignore_ascii_case("mock") {
            return Self::mock();
        }
        Self {
            clients: WorkflowClients::from_env(),
        }
    }

    /// Run the workflow and persist results.
    pub fn run(
        &self,
        pool: &DbPool,
        vessel_id: &str,
        request: &WorkflowRequest,
    ) -> Result<WorkflowResult, WorkflowError> {
        let runner = WorkflowRunner::new(self.clients.clone());
        let result = runner.run(request);
        persist_workflow(pool, vessel_id, &result)?;
        Ok(result)
    }
}

/// Orchestrates the GitHub PR + GitLab mirror workflow.
#[derive(Clone)]
pub struct WorkflowRunner {
    clients: WorkflowClients,
}

impl WorkflowRunner {
    /// Build a workflow runner with mock clients.
    #[cfg(test)]
    pub fn mock() -> Self {
        Self::new(WorkflowClients::mock())
    }

    /// Build a workflow runner with explicit clients.
    pub fn new(clients: WorkflowClients) -> Self {
        Self { clients }
    }

    /// Execute the workflow with the configured clients.
    pub fn run(&self, request: &WorkflowRequest) -> WorkflowResult {
        let workflow_id = Uuid::new_v4().to_string();
        let mut steps = Vec::new();
        let mut overall_status = WorkflowStatus::Success;
        let mut pr_url = None;
        let mut pipeline_url = None;

        let workspace = match self.clients.git.prepare_workspace(&request.repo) {
            Ok(workspace) => workspace,
            Err(err) => {
                steps.push(step_failed(WorkflowStepKind::ApplyPatch, err.to_string()));
                overall_status = WorkflowStatus::Failed;
                push_skipped_steps(&mut steps, next_steps(WorkflowStepKind::ApplyPatch));
                return WorkflowResult {
                    workflow_id,
                    status: overall_status,
                    steps,
                    pr_url,
                    pipeline_url,
                };
            }
        };

        let commit = match self.clients.git.apply_patch(&workspace, &request.patch) {
            Ok(commit) => {
                steps.push(step_success(
                    WorkflowStepKind::ApplyPatch,
                    Some(format!("applied patch ({commit})")),
                ));
                commit
            }
            Err(err) => {
                steps.push(step_failed(WorkflowStepKind::ApplyPatch, err.to_string()));
                overall_status = WorkflowStatus::Failed;
                push_skipped_steps(&mut steps, next_steps(WorkflowStepKind::ApplyPatch));
                let _ = self.clients.git.cleanup(workspace);
                return WorkflowResult {
                    workflow_id,
                    status: overall_status,
                    steps,
                    pr_url,
                    pipeline_url,
                };
            }
        };

        if let Err(err) = self.clients.git.create_branch(
            &workspace,
            &request.repo.base_branch,
            &request.patch.branch,
        ) {
            steps.push(step_failed(WorkflowStepKind::CreateBranch, err.to_string()));
            overall_status = WorkflowStatus::Failed;
            push_skipped_steps(&mut steps, next_steps(WorkflowStepKind::CreateBranch));
            let _ = self.clients.git.cleanup(workspace);
            return WorkflowResult {
                workflow_id,
                status: overall_status,
                steps,
                pr_url,
                pipeline_url,
            };
        } else {
            steps.push(step_success(
                WorkflowStepKind::CreateBranch,
                Some(format!(
                    "created branch {} from {}",
                    request.patch.branch, request.repo.base_branch
                )),
            ));
        }

        match self
            .clients
            .git
            .push_branch(&workspace, &request.repo, &request.patch.branch)
        {
            Ok(remote_ref) => {
                steps.push(step_success(
                    WorkflowStepKind::PushBranch,
                    Some(format!("pushed {remote_ref} (commit {commit})")),
                ));
            }
            Err(err) => {
                steps.push(step_failed(WorkflowStepKind::PushBranch, err.to_string()));
                overall_status = WorkflowStatus::Failed;
                push_skipped_steps(&mut steps, next_steps(WorkflowStepKind::PushBranch));
                let _ = self.clients.git.cleanup(workspace);
                return WorkflowResult {
                    workflow_id,
                    status: overall_status,
                    steps,
                    pr_url,
                    pipeline_url,
                };
            }
        }

        match self.clients.gitlab.ensure_project(&request.gitlab) {
            Ok(()) => {
                steps.push(step_success(
                    WorkflowStepKind::EnsureGitlabProject,
                    Some(format!(
                        "ensured GitLab project {}",
                        request.gitlab.project_path
                    )),
                ));
            }
            Err(err) => {
                steps.push(step_failed(
                    WorkflowStepKind::EnsureGitlabProject,
                    err.to_string(),
                ));
                overall_status = WorkflowStatus::Failed;
                push_skipped_steps(
                    &mut steps,
                    next_steps(WorkflowStepKind::EnsureGitlabProject),
                );
                let _ = self.clients.git.cleanup(workspace);
                return WorkflowResult {
                    workflow_id,
                    status: overall_status,
                    steps,
                    pr_url,
                    pipeline_url,
                };
            }
        }

        match self
            .clients
            .git
            .push_mirror(&workspace, &request.gitlab, &request.patch.branch)
        {
            Ok(remote_ref) => {
                steps.push(step_success(
                    WorkflowStepKind::MirrorPush,
                    Some(format!("pushed {remote_ref}")),
                ));
            }
            Err(err) => {
                steps.push(step_failed(WorkflowStepKind::MirrorPush, err.to_string()));
                overall_status = WorkflowStatus::Failed;
                push_skipped_steps(&mut steps, next_steps(WorkflowStepKind::MirrorPush));
                let _ = self.clients.git.cleanup(workspace);
                return WorkflowResult {
                    workflow_id,
                    status: overall_status,
                    steps,
                    pr_url,
                    pipeline_url,
                };
            }
        }

        match self
            .clients
            .gitlab
            .trigger_pipeline(&request.gitlab, &request.patch.branch)
        {
            Ok(url) => {
                pipeline_url = Some(url.clone());
                steps.push(step_success(
                    WorkflowStepKind::TriggerGitlab,
                    Some(format!("triggered pipeline {url}")),
                ));
            }
            Err(err) => {
                steps.push(step_failed(
                    WorkflowStepKind::TriggerGitlab,
                    err.to_string(),
                ));
                overall_status = WorkflowStatus::Failed;
                push_skipped_steps(&mut steps, next_steps(WorkflowStepKind::TriggerGitlab));
                let _ = self.clients.git.cleanup(workspace);
                return WorkflowResult {
                    workflow_id,
                    status: overall_status,
                    steps,
                    pr_url,
                    pipeline_url,
                };
            }
        }

        let pr_spec = match build_pr_spec(request, &workspace) {
            Ok(pr_spec) => pr_spec,
            Err(err) => {
                steps.push(step_failed(WorkflowStepKind::OpenPr, err.to_string()));
                overall_status = WorkflowStatus::Failed;
                let _ = self.clients.git.cleanup(workspace);
                return WorkflowResult {
                    workflow_id,
                    status: overall_status,
                    steps,
                    pr_url,
                    pipeline_url,
                };
            }
        };

        match self
            .clients
            .github
            .open_pr(&request.repo, &pr_spec, &request.patch.branch)
        {
            Ok(url) => {
                pr_url = Some(url.clone());
                steps.push(step_success(
                    WorkflowStepKind::OpenPr,
                    Some(format!("opened PR {url}")),
                ));
            }
            Err(err) => {
                steps.push(step_failed(WorkflowStepKind::OpenPr, err.to_string()));
                overall_status = WorkflowStatus::Failed;
            }
        }

        let _ = self.clients.git.cleanup(workspace);

        WorkflowResult {
            workflow_id,
            status: overall_status,
            steps,
            pr_url,
            pipeline_url,
        }
    }
}

fn build_pr_spec(
    request: &WorkflowRequest,
    workspace: &GitWorkspace,
) -> Result<PullRequestSpec, WorkflowError> {
    let mut pr = request.pr.clone();
    let Some(report) = request.fleet_report.as_ref() else {
        return Ok(pr);
    };
    let fs = StdFileSystem::new();
    let context = PrTemplateContext::from_report(report);
    match interpolate_pr_template(&fs, &workspace.path, &context) {
        Ok(Some(rendered)) => {
            let extra = pr.body.as_deref();
            pr.body = Some(merge_pr_body(rendered, extra));
        }
        Ok(None) => {}
        Err(err) => {
            return Err(WorkflowError::new(format!(
                "pr template interpolation failed: {err}"
            )));
        }
    }
    Ok(pr)
}

fn merge_pr_body(template: String, extra: Option<&str>) -> String {
    let extra = extra.map(str::trim).filter(|value| !value.is_empty());
    match extra {
        Some(extra) => format!("{template}\n\n{extra}"),
        None => template,
    }
}

fn persist_workflow(
    pool: &DbPool,
    vessel_id: &str,
    result: &WorkflowResult,
) -> Result<(), WorkflowError> {
    let mut conn = pool
        .get()
        .map_err(|err| WorkflowError::new(format!("db connection failed: {err}")))?;
    let now = Utc::now().naive_utc();
    let record = NewWorkflow {
        id: result.workflow_id.clone(),
        vessel_id: vessel_id.to_string(),
        status: result.status.as_str().to_string(),
        pr_url: result.pr_url.clone(),
        pipeline_url: result.pipeline_url.clone(),
        created_at: now,
    };
    diesel::insert_into(workflows::table)
        .values(&record)
        .execute(&mut conn)
        .map_err(|err| WorkflowError::new(format!("persist workflow failed: {err}")))?;

    let steps: Vec<NewWorkflowStep> = result
        .steps
        .iter()
        .map(|step| NewWorkflowStep {
            id: Uuid::new_v4().to_string(),
            workflow_id: result.workflow_id.clone(),
            kind: step.kind.as_str().to_string(),
            status: step.status.as_str().to_string(),
            detail: step.detail.clone(),
            created_at: now,
        })
        .collect();
    if !steps.is_empty() {
        diesel::insert_into(workflow_steps::table)
            .values(&steps)
            .execute(&mut conn)
            .map_err(|err| WorkflowError::new(format!("persist workflow steps failed: {err}")))?;
    }

    Ok(())
}

fn step_success(kind: WorkflowStepKind, detail: Option<String>) -> WorkflowStep {
    WorkflowStep {
        kind,
        status: WorkflowStatus::Success,
        detail,
    }
}

fn step_failed(kind: WorkflowStepKind, detail: String) -> WorkflowStep {
    WorkflowStep {
        kind,
        status: WorkflowStatus::Failed,
        detail: Some(detail),
    }
}

fn step_skipped(kind: WorkflowStepKind) -> WorkflowStep {
    WorkflowStep {
        kind,
        status: WorkflowStatus::Skipped,
        detail: Some("skipped due to prior failure".to_string()),
    }
}

fn next_steps(start: WorkflowStepKind) -> &'static [WorkflowStepKind] {
    match start {
        WorkflowStepKind::ApplyPatch => &[
            WorkflowStepKind::CreateBranch,
            WorkflowStepKind::PushBranch,
            WorkflowStepKind::EnsureGitlabProject,
            WorkflowStepKind::MirrorPush,
            WorkflowStepKind::TriggerGitlab,
            WorkflowStepKind::OpenPr,
        ],
        WorkflowStepKind::CreateBranch => &[
            WorkflowStepKind::PushBranch,
            WorkflowStepKind::EnsureGitlabProject,
            WorkflowStepKind::MirrorPush,
            WorkflowStepKind::TriggerGitlab,
            WorkflowStepKind::OpenPr,
        ],
        WorkflowStepKind::PushBranch => &[
            WorkflowStepKind::EnsureGitlabProject,
            WorkflowStepKind::MirrorPush,
            WorkflowStepKind::TriggerGitlab,
            WorkflowStepKind::OpenPr,
        ],
        WorkflowStepKind::EnsureGitlabProject => &[
            WorkflowStepKind::MirrorPush,
            WorkflowStepKind::TriggerGitlab,
            WorkflowStepKind::OpenPr,
        ],
        WorkflowStepKind::MirrorPush => {
            &[WorkflowStepKind::TriggerGitlab, WorkflowStepKind::OpenPr]
        }
        WorkflowStepKind::TriggerGitlab => &[WorkflowStepKind::OpenPr],
        WorkflowStepKind::OpenPr => &[],
    }
}

fn push_skipped_steps(steps: &mut Vec<WorkflowStep>, remaining: &[WorkflowStepKind]) {
    for kind in remaining {
        steps.push(step_skipped(*kind));
    }
}

fn parse_github_repo(url: &str) -> Result<(String, String), WorkflowError> {
    let trimmed = url.trim().trim_end_matches(".git");
    let url = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("git@github.com:"))
        .ok_or_else(|| WorkflowError::new("unsupported github repo url"))?;
    let mut parts = url.split('/');
    let owner = parts
        .next()
        .ok_or_else(|| WorkflowError::new("missing github owner"))?;
    let repo = parts
        .next()
        .ok_or_else(|| WorkflowError::new("missing github repo"))?;
    Ok((owner.to_string(), repo.to_string()))
}

fn split_gitlab_project_path(
    project_path: &str,
) -> Result<(Option<String>, String), WorkflowError> {
    let trimmed = project_path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(WorkflowError::new("gitlab project_path is required"));
    }
    match trimmed.rsplit_once('/') {
        Some((namespace, name)) => {
            let namespace = namespace.trim().trim_end_matches('/');
            let name = name.trim();
            if name.is_empty() {
                return Err(WorkflowError::new(
                    "gitlab project_path missing project name",
                ));
            }
            let namespace = if namespace.is_empty() {
                None
            } else {
                Some(namespace.to_string())
            };
            Ok((namespace, name.to_string()))
        }
        None => Ok((None, trimmed.to_string())),
    }
}

fn maybe_inject_token(url: &str, token: Option<&str>, username: &str) -> String {
    let Some(token) = token else {
        return url.to_string();
    };
    if !url.starts_with("https://") {
        return url.to_string();
    }
    let mut rest = url.trim_start_matches("https://").to_string();
    if rest.contains('@') {
        return url.to_string();
    }
    rest.insert_str(0, &format!("{username}:{token}@"));
    format!("https://{rest}")
}

fn run_git(path: &Path, args: &[&str], envs: &[(&str, &str)]) -> Result<String, WorkflowError> {
    let mut command = Command::new("git");
    command.args(args).current_dir(path);
    command.env("GIT_TERMINAL_PROMPT", "0");
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .map_err(|err| WorkflowError::new(format!("git command failed: {err}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(WorkflowError::new(format!(
            "git {:?} failed: {}",
            args, detail
        )));
    }
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;
    use shipshape_core::{CoverageReport, Violation};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Mutex, OnceLock};

    fn sample_request() -> WorkflowRequest {
        WorkflowRequest {
            repo: RepoSpec {
                repo_url: "https://github.com/shipshape/shipshape-demo.git".to_string(),
                base_branch: "main".to_string(),
                local_path: None,
            },
            patch: PatchSpec {
                diff: "diff --git a/README.md b/README.md\n".to_string(),
                branch: "shipshape/refit".to_string(),
                commit_message: "Apply ShipShape refit".to_string(),
            },
            pr: PullRequestSpec {
                title: "ShipShape refit".to_string(),
                body: Some("Automated refit results.".to_string()),
                draft: true,
            },
            fleet_report: None,
            gitlab: GitLabSpec {
                mirror_url: "https://gitlab.example.com/shipshape/shipshape-demo.git".to_string(),
                project_path: "shipshape/shipshape-demo".to_string(),
                pipeline_ref: None,
            },
        }
    }

    #[test]
    fn workflow_runs_successfully() {
        let runner = WorkflowRunner::mock();
        let result = runner.run(&sample_request());

        assert_eq!(result.status, WorkflowStatus::Success);
        assert_eq!(result.steps.len(), 7);
        assert!(result.pr_url.is_some());
        assert!(result.pipeline_url.is_some());
    }

    #[test]
    fn workflow_fails_when_patch_missing() {
        let runner = WorkflowRunner::mock();
        let mut request = sample_request();
        request.patch.diff = String::new();

        let result = runner.run(&request);

        assert_eq!(result.status, WorkflowStatus::Failed);
        assert_eq!(result.steps.first().unwrap().status, WorkflowStatus::Failed);
        assert_eq!(result.steps.len(), 7);
        assert!(
            result
                .steps
                .iter()
                .skip(1)
                .all(|step| step.status == WorkflowStatus::Skipped)
        );
    }

    #[test]
    fn status_and_step_labels_are_stable() {
        assert_eq!(WorkflowStatus::Success.as_str(), "success");
        assert_eq!(WorkflowStatus::Failed.as_str(), "failed");
        assert_eq!(WorkflowStatus::Skipped.as_str(), "skipped");

        assert_eq!(WorkflowStepKind::ApplyPatch.as_str(), "apply_patch");
        assert_eq!(WorkflowStepKind::CreateBranch.as_str(), "create_branch");
        assert_eq!(WorkflowStepKind::PushBranch.as_str(), "push_branch");
        assert_eq!(
            WorkflowStepKind::EnsureGitlabProject.as_str(),
            "ensure_gitlab_project"
        );
        assert_eq!(WorkflowStepKind::MirrorPush.as_str(), "mirror_push");
        assert_eq!(WorkflowStepKind::TriggerGitlab.as_str(), "trigger_gitlab");
        assert_eq!(WorkflowStepKind::OpenPr.as_str(), "open_pr");
    }

    #[test]
    fn parse_github_repo_accepts_common_forms() {
        let (owner, repo) = parse_github_repo("https://github.com/shipshape/demo.git").unwrap();
        assert_eq!(owner, "shipshape");
        assert_eq!(repo, "demo");

        let (owner, repo) = parse_github_repo("http://github.com/shipshape/demo").unwrap();
        assert_eq!(owner, "shipshape");
        assert_eq!(repo, "demo");

        let (owner, repo) = parse_github_repo("git@github.com:shipshape/demo.git").unwrap();
        assert_eq!(owner, "shipshape");
        assert_eq!(repo, "demo");

        assert!(parse_github_repo("https://example.com/other").is_err());
    }

    #[test]
    fn split_gitlab_project_path_parses_variants() {
        let (namespace, name) = split_gitlab_project_path("demo").expect("parse");
        assert!(namespace.is_none());
        assert_eq!(name, "demo");

        let (namespace, name) = split_gitlab_project_path("shipshape/demo").expect("parse");
        assert_eq!(namespace.as_deref(), Some("shipshape"));
        assert_eq!(name, "demo");

        let err = split_gitlab_project_path("shipshape/").unwrap_err();
        assert!(err.to_string().contains("missing project name"));
    }

    #[test]
    fn maybe_inject_token_behaves_safely() {
        assert_eq!(
            maybe_inject_token("https://github.com/org/repo", None, "x"),
            "https://github.com/org/repo"
        );
        assert_eq!(
            maybe_inject_token("git@github.com:org/repo", Some("tok"), "x"),
            "git@github.com:org/repo"
        );
        assert_eq!(
            maybe_inject_token("https://tok@github.com/org/repo", Some("tok"), "x"),
            "https://tok@github.com/org/repo"
        );
        assert_eq!(
            maybe_inject_token("https://github.com/org/repo", Some("tok"), "x"),
            "https://x:tok@github.com/org/repo"
        );
    }

    #[test]
    fn mock_clients_validate_inputs() {
        let git = MockGitClient;
        let repo = RepoSpec {
            repo_url: "".to_string(),
            base_branch: "".to_string(),
            local_path: None,
        };
        assert!(git.prepare_workspace(&repo).is_err());

        let github = MockGitHubClient;
        let pr = PullRequestSpec {
            title: "".to_string(),
            body: None,
            draft: false,
        };
        assert!(github.open_pr(&repo, &pr, "branch").is_err());

        let gitlab = MockGitLabClient;
        let gitlab_spec = GitLabSpec {
            mirror_url: "https://gitlab.example.com/org/repo.git".to_string(),
            project_path: "".to_string(),
            pipeline_ref: None,
        };
        assert!(gitlab.ensure_project(&gitlab_spec).is_err());
        assert!(gitlab.trigger_pipeline(&gitlab_spec, "branch").is_err());
    }

    #[test]
    fn workflow_interpolates_pr_template() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("PULL_REQUEST_TEMPLATE.md"),
            format!(
                "{}\n{}\n{}\n",
                shipshape_core::SHIPSHAPE_STATS,
                shipshape_core::SHIPSHAPE_FIXES,
                shipshape_core::SHIPSHAPE_CI
            ),
        )
        .expect("write template");

        let git = Arc::new(TemplateGitClient { root: root.clone() });
        let github = Arc::new(RecordingGitHubClient::default());
        let gitlab = Arc::new(MockGitLabClient);
        let runner = WorkflowRunner::new(WorkflowClients {
            git,
            github: github.clone(),
            gitlab,
        });

        let mut request = sample_request();
        request.fleet_report = Some(sample_report());
        request.pr.body = Some("Extra notes.".to_string());

        let result = runner.run(&request);

        assert_eq!(result.status, WorkflowStatus::Success);
        let body = github
            .last_body
            .lock()
            .expect("lock")
            .clone()
            .expect("body");
        assert!(body.contains("ShipShape stats:"));
        assert!(body.contains("Extra notes."));
        assert!(!body.contains(shipshape_core::SHIPSHAPE_STATS));
        assert!(!body.contains(shipshape_core::SHIPSHAPE_FIXES));
        assert!(!body.contains(shipshape_core::SHIPSHAPE_CI));

        cleanup_dir(root);
    }

    #[test]
    fn workflow_uses_request_body_without_template() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).expect("create temp dir");

        let git = Arc::new(TemplateGitClient { root: root.clone() });
        let github = Arc::new(RecordingGitHubClient::default());
        let gitlab = Arc::new(MockGitLabClient);
        let runner = WorkflowRunner::new(WorkflowClients {
            git,
            github: github.clone(),
            gitlab,
        });

        let mut request = sample_request();
        request.fleet_report = Some(sample_report());
        request.pr.body = Some("Original body.".to_string());

        let result = runner.run(&request);

        assert_eq!(result.status, WorkflowStatus::Success);
        let body = github
            .last_body
            .lock()
            .expect("lock")
            .clone()
            .expect("body");
        assert_eq!(body, "Original body.");

        cleanup_dir(root);
    }

    #[derive(Debug)]
    struct TemplateGitClient {
        root: PathBuf,
    }

    impl GitClient for TemplateGitClient {
        fn prepare_workspace(&self, repo: &RepoSpec) -> Result<GitWorkspace, WorkflowError> {
            if repo.repo_url.trim().is_empty() {
                return Err(WorkflowError::new("repo_url is required"));
            }
            if repo.base_branch.trim().is_empty() {
                return Err(WorkflowError::new("base_branch is required"));
            }
            Ok(GitWorkspace {
                path: self.root.clone(),
                managed: false,
            })
        }

        fn apply_patch(
            &self,
            _workspace: &GitWorkspace,
            patch: &PatchSpec,
        ) -> Result<String, WorkflowError> {
            if patch.diff.trim().is_empty() {
                return Err(WorkflowError::new("patch diff is empty"));
            }
            Ok("mock-commit-sha".to_string())
        }

        fn create_branch(
            &self,
            _workspace: &GitWorkspace,
            base_branch: &str,
            branch: &str,
        ) -> Result<(), WorkflowError> {
            if base_branch.trim().is_empty() {
                return Err(WorkflowError::new("base branch is required"));
            }
            if branch.trim().is_empty() {
                return Err(WorkflowError::new("branch name is required"));
            }
            Ok(())
        }

        fn push_branch(
            &self,
            _workspace: &GitWorkspace,
            repo: &RepoSpec,
            branch: &str,
        ) -> Result<String, WorkflowError> {
            if repo.repo_url.trim().is_empty() {
                return Err(WorkflowError::new("repo_url is required"));
            }
            Ok(format!("origin/{branch}"))
        }

        fn push_mirror(
            &self,
            _workspace: &GitWorkspace,
            gitlab: &GitLabSpec,
            branch: &str,
        ) -> Result<String, WorkflowError> {
            if gitlab.mirror_url.trim().is_empty() {
                return Err(WorkflowError::new("gitlab mirror_url is required"));
            }
            Ok(format!("mirror/{branch}"))
        }

        fn cleanup(&self, _workspace: GitWorkspace) -> Result<(), WorkflowError> {
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct RecordingGitHubClient {
        last_body: Mutex<Option<String>>,
    }

    impl GitHubClient for RecordingGitHubClient {
        fn open_pr(
            &self,
            repo: &RepoSpec,
            pr: &PullRequestSpec,
            _branch: &str,
        ) -> Result<String, WorkflowError> {
            if repo.repo_url.trim().is_empty() {
                return Err(WorkflowError::new("repo_url is required"));
            }
            if pr.title.trim().is_empty() {
                return Err(WorkflowError::new("pull request title is required"));
            }
            let mut guard = self.last_body.lock().expect("lock");
            *guard = pr.body.clone();
            Ok("https://github.com/shipshape/mock/pull/1".to_string())
        }
    }

    fn sample_report() -> FleetReport {
        let mut language_stats = BTreeMap::new();
        language_stats.insert("Rust".to_string(), 70.0);
        language_stats.insert("Go".to_string(), 30.0);
        FleetReport {
            language_stats,
            violations: vec![Violation {
                id: "doc-1".to_string(),
                message: "Missing docs".to_string(),
            }],
            coverage: CoverageReport {
                code_files: 10,
                test_files: 5,
                doc_files: 2,
                test_coverage: 0.5,
                doc_coverage: 0.2,
                low_test_coverage: false,
                low_doc_coverage: true,
            },
            health_score: 84,
        }
    }

    fn cleanup_dir(root: PathBuf) {
        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    #[test]
    fn git_command_client_validates_local_paths() {
        let client = GitCommandClient::from_env();
        let repo = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: "".to_string(),
            local_path: Some(temp_dir().to_str().unwrap().to_string()),
        };
        let result = client.prepare_workspace(&repo);
        assert!(result.is_err());

        let repo = RepoSpec {
            repo_url: "".to_string(),
            base_branch: "main".to_string(),
            local_path: None,
        };
        let result = client.prepare_workspace(&repo);
        assert!(result.is_err());

        let root = temp_dir();
        std::fs::create_dir_all(&root).expect("create dir");
        let repo = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: "main".to_string(),
            local_path: Some(root.to_str().unwrap().to_string()),
        };
        let result = client.prepare_workspace(&repo);
        assert!(result.is_err());

        std::fs::remove_dir_all(&root).expect("cleanup");

        let git_root = temp_dir();
        std::fs::create_dir_all(&git_root).expect("create git dir");
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&git_root)
            .status()
            .expect("git init");
        let repo = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: "main".to_string(),
            local_path: Some(git_root.to_str().unwrap().to_string()),
        };
        let result = client.prepare_workspace(&repo).expect("workspace");
        assert!(!result.managed);
        std::fs::remove_dir_all(&git_root).expect("cleanup git dir");
    }

    #[test]
    fn git_command_client_runs_full_flow() {
        let _guard = env_lock();
        let origin = init_bare_repo();
        let mirror = init_bare_repo();
        let work = init_work_repo(&origin);
        let workspace_root = temp_dir();
        set_env("SHIPSHAPE_WORKSPACE_ROOT", workspace_root.to_str().unwrap());

        let client = GitCommandClient::from_env();
        let repo = RepoSpec {
            repo_url: origin.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            local_path: None,
        };
        let workspace = client.prepare_workspace(&repo).expect("workspace");

        let patch = PatchSpec {
            diff: make_patch(&work),
            branch: "shipshape/refit".to_string(),
            commit_message: "Apply patch".to_string(),
        };
        let commit = client.apply_patch(&workspace, &patch).expect("apply patch");
        assert!(!commit.is_empty());

        client
            .create_branch(&workspace, &repo.base_branch, &patch.branch)
            .expect("create branch");

        let remote_ref = client
            .push_branch(&workspace, &repo, &patch.branch)
            .expect("push branch");
        assert!(remote_ref.contains(&patch.branch));

        let gitlab = GitLabSpec {
            mirror_url: mirror.to_str().unwrap().to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };
        let mirror_ref = client
            .push_mirror(&workspace, &gitlab, &patch.branch)
            .expect("push mirror");
        assert!(mirror_ref.contains(&patch.branch));

        client.cleanup(workspace).expect("cleanup");

        std::fs::remove_dir_all(&origin).expect("cleanup origin");
        std::fs::remove_dir_all(&mirror).expect("cleanup mirror");
        std::fs::remove_dir_all(&work).expect("cleanup work");
        std::fs::remove_dir_all(&workspace_root).expect("cleanup workspace");
        remove_env("SHIPSHAPE_WORKSPACE_ROOT");
    }

    #[test]
    fn git_command_client_rejects_empty_inputs() {
        let client = GitCommandClient::from_env();
        let workspace = GitWorkspace {
            path: PathBuf::from("/tmp"),
            managed: false,
        };
        let patch = PatchSpec {
            diff: String::new(),
            branch: "branch".to_string(),
            commit_message: "msg".to_string(),
        };
        assert!(client.apply_patch(&workspace, &patch).is_err());
        assert!(client.create_branch(&workspace, "main", "").is_err());

        let gitlab = GitLabSpec {
            mirror_url: "".to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };
        assert!(client.push_mirror(&workspace, &gitlab, "branch").is_err());
    }

    #[test]
    fn github_api_client_handles_success_and_errors() {
        let _guard = env_lock();
        let repo = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: "main".to_string(),
            local_path: None,
        };
        let pr = PullRequestSpec {
            title: "ShipShape".to_string(),
            body: None,
            draft: false,
        };

        {
            let server = MockServer::start();
            let github = server.mock(|when, then| {
                when.method(POST).path("/repos/shipshape/demo/pulls");
                then.status(201)
                    .header("content-type", "application/json")
                    .json_body(
                        serde_json::json!({"html_url": "https://github.com/shipshape/demo/pull/1"}),
                    );
            });

            set_env("GITHUB_TOKEN", "token");
            set_env("GITHUB_API_URL", server.url("").as_str());
            set_env("GITHUB_USER_AGENT", "shipshape-test");
            let client = GitHubApiClient::from_env();
            let result = client.open_pr(&repo, &pr, "branch").expect("open pr");
            assert!(result.contains("pull/1"));
            github.assert();
        }

        {
            let server = MockServer::start();
            let error_mock = server.mock(|when, then| {
                when.method(POST).path("/repos/shipshape/demo/pulls");
                then.status(400).body("bad");
            });

            set_env("GITHUB_TOKEN", "token");
            set_env("GITHUB_API_URL", server.url("").as_str());
            set_env("GITHUB_USER_AGENT", "shipshape-test");
            let client = GitHubApiClient::from_env();
            let err = client.open_pr(&repo, &pr, "branch").unwrap_err();
            assert!(err.to_string().contains("github api error"));
            error_mock.assert();
        }

        {
            let server = MockServer::start();
            let missing_mock = server.mock(|when, then| {
                when.method(POST).path("/repos/shipshape/demo/pulls");
                then.status(201)
                    .header("content-type", "application/json")
                    .json_body(serde_json::json!({"id": 1}));
            });

            set_env("GITHUB_TOKEN", "token");
            set_env("GITHUB_API_URL", server.url("").as_str());
            set_env("GITHUB_USER_AGENT", "shipshape-test");
            let client = GitHubApiClient::from_env();
            let err = client.open_pr(&repo, &pr, "branch").unwrap_err();
            assert!(err.to_string().contains("missing html_url"));
            missing_mock.assert();
        }

        remove_env("GITHUB_TOKEN");
        remove_env("GITHUB_API_URL");
        remove_env("GITHUB_USER_AGENT");
    }

    #[test]
    fn github_api_client_requires_token() {
        let _guard = env_lock();
        remove_env("GITHUB_TOKEN");
        let client = GitHubApiClient::from_env();
        let repo = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: "main".to_string(),
            local_path: None,
        };
        let pr = PullRequestSpec {
            title: "ShipShape".to_string(),
            body: None,
            draft: false,
        };
        let err = client.open_pr(&repo, &pr, "branch").unwrap_err();
        assert!(err.to_string().contains("GITHUB_TOKEN is required"));
    }

    #[test]
    fn gitlab_api_client_handles_success_and_errors() {
        let _guard = env_lock();
        let gitlab = GitLabSpec {
            mirror_url: "https://gitlab.example.com/shipshape/demo.git".to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };

        {
            let server = MockServer::start();
            let ok = server.mock(|when, then| {
                when.method(POST).path("/api/v4/projects/shipshape%2Fdemo/pipeline");
                then.status(201).header("content-type", "application/json").json_body(
                    serde_json::json!({"web_url": "https://gitlab.example.com/shipshape/demo/-/pipelines/1"}),
                );
            });

            set_env("GITLAB_TOKEN", "token");
            set_env("GITLAB_API_URL", server.url("").as_str());
            let client = GitLabApiClient::from_env();
            let result = client
                .trigger_pipeline(&gitlab, "branch")
                .expect("pipeline");
            assert!(result.contains("pipelines/1"));
            ok.assert();
        }

        {
            let server = MockServer::start();
            let fallback = server.mock(|when, then| {
                when.method(POST)
                    .path("/api/v4/projects/shipshape%2Fdemo/pipeline");
                then.status(201)
                    .header("content-type", "application/json")
                    .json_body(serde_json::json!({"id": 22}));
            });

            set_env("GITLAB_TOKEN", "token");
            set_env("GITLAB_API_URL", server.url("").as_str());
            let client = GitLabApiClient::from_env();
            let result = client
                .trigger_pipeline(&gitlab, "branch")
                .expect("pipeline");
            assert!(result.contains("pipelines/22"));
            fallback.assert();
        }

        {
            let server = MockServer::start();
            let error = server.mock(|when, then| {
                when.method(POST)
                    .path("/api/v4/projects/shipshape%2Fdemo/pipeline");
                then.status(400).body("bad");
            });

            set_env("GITLAB_TOKEN", "token");
            set_env("GITLAB_API_URL", server.url("").as_str());
            let client = GitLabApiClient::from_env();
            let err = client.trigger_pipeline(&gitlab, "branch").unwrap_err();
            assert!(err.to_string().contains("gitlab api error"));
            error.assert();
        }

        {
            let server = MockServer::start();
            let missing = server.mock(|when, then| {
                when.method(POST)
                    .path("/api/v4/projects/shipshape%2Fdemo/pipeline");
                then.status(201)
                    .header("content-type", "application/json")
                    .json_body(serde_json::json!({}));
            });

            set_env("GITLAB_TOKEN", "token");
            set_env("GITLAB_API_URL", server.url("").as_str());
            let client = GitLabApiClient::from_env();
            let err = client.trigger_pipeline(&gitlab, "branch").unwrap_err();
            assert!(err.to_string().contains("missing web_url"));
            missing.assert();
        }

        remove_env("GITLAB_TOKEN");
        remove_env("GITLAB_API_URL");
    }

    #[test]
    fn gitlab_api_client_ensures_project_exists() {
        let _guard = env_lock();
        let gitlab = GitLabSpec {
            mirror_url: "https://gitlab.example.com/shipshape/demo.git".to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };

        let server = MockServer::start();
        let lookup = server.mock(|when, then| {
            when.method(GET).path("/api/v4/projects/shipshape%2Fdemo");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"id": 1}));
        });
        let create = server.mock(|when, then| {
            when.method(POST).path("/api/v4/projects");
            then.status(500).body("unexpected create");
        });

        set_env("GITLAB_TOKEN", "token");
        set_env("GITLAB_API_URL", server.url("").as_str());
        let client = GitLabApiClient::from_env();
        client.ensure_project(&gitlab).expect("ensure project");
        lookup.assert();
        create.assert_hits(0);

        remove_env("GITLAB_TOKEN");
        remove_env("GITLAB_API_URL");
    }

    #[test]
    fn gitlab_api_client_creates_missing_project() {
        let _guard = env_lock();
        let gitlab = GitLabSpec {
            mirror_url: "https://gitlab.example.com/shipshape/demo.git".to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };

        let server = MockServer::start();
        let lookup = server.mock(|when, then| {
            when.method(GET).path("/api/v4/projects/shipshape%2Fdemo");
            then.status(404);
        });
        let namespace = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v4/namespaces")
                .query_param("search", "shipshape");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!([{
                    "id": 42,
                    "full_path": "shipshape"
                }]));
        });
        let create = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v4/projects")
                .body_contains("name=demo")
                .body_contains("path=demo")
                .body_contains("namespace_id=42");
            then.status(201)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"id": 99}));
        });

        set_env("GITLAB_TOKEN", "token");
        set_env("GITLAB_API_URL", server.url("").as_str());
        let client = GitLabApiClient::from_env();
        client.ensure_project(&gitlab).expect("ensure project");
        lookup.assert();
        namespace.assert();
        create.assert();

        remove_env("GITLAB_TOKEN");
        remove_env("GITLAB_API_URL");
    }

    #[test]
    fn gitlab_api_client_reports_project_lookup_errors() {
        let _guard = env_lock();
        let gitlab = GitLabSpec {
            mirror_url: "https://gitlab.example.com/shipshape/demo.git".to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };

        let server = MockServer::start();
        let lookup = server.mock(|when, then| {
            when.method(GET).path("/api/v4/projects/shipshape%2Fdemo");
            then.status(500).body("boom");
        });

        set_env("GITLAB_TOKEN", "token");
        set_env("GITLAB_API_URL", server.url("").as_str());
        let client = GitLabApiClient::from_env();
        let err = client.ensure_project(&gitlab).unwrap_err();
        assert!(err.to_string().contains("gitlab api error"));
        lookup.assert();

        remove_env("GITLAB_TOKEN");
        remove_env("GITLAB_API_URL");
    }

    #[test]
    fn gitlab_api_client_requires_token_and_project() {
        let _guard = env_lock();
        remove_env("GITLAB_TOKEN");
        let client = GitLabApiClient::from_env();
        let gitlab = GitLabSpec {
            mirror_url: "https://gitlab.example.com/shipshape/demo.git".to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };
        let err = client.ensure_project(&gitlab).unwrap_err();
        assert!(err.to_string().contains("GITLAB_TOKEN is required"));
        let err = client.trigger_pipeline(&gitlab, "branch").unwrap_err();
        assert!(err.to_string().contains("GITLAB_TOKEN is required"));

        set_env("GITLAB_TOKEN", "token");
        let client = GitLabApiClient::from_env();
        let gitlab = GitLabSpec {
            mirror_url: "https://gitlab.example.com/shipshape/demo.git".to_string(),
            project_path: "".to_string(),
            pipeline_ref: None,
        };
        let err = client.ensure_project(&gitlab).unwrap_err();
        assert!(err.to_string().contains("project_path is required"));
        let err = client.trigger_pipeline(&gitlab, "branch").unwrap_err();
        assert!(err.to_string().contains("project_path is required"));
        remove_env("GITLAB_TOKEN");
    }

    #[test]
    fn workflow_runner_fails_at_each_step() {
        let request = sample_request();

        let runner = WorkflowRunner::new(WorkflowClients {
            git: Arc::new(FailingGitClient::new(WorkflowStepKind::CreateBranch)),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(MockGitLabClient),
        });
        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Failed);
        assert!(
            result
                .steps
                .iter()
                .skip(2)
                .all(|step| step.status == WorkflowStatus::Skipped)
        );

        let runner = WorkflowRunner::new(WorkflowClients {
            git: Arc::new(FailingGitClient::new(WorkflowStepKind::PushBranch)),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(MockGitLabClient),
        });
        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Failed);
        assert!(
            result
                .steps
                .iter()
                .skip(3)
                .all(|step| step.status == WorkflowStatus::Skipped)
        );

        let runner = WorkflowRunner::new(WorkflowClients {
            git: Arc::new(MockGitClient),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(FailingGitLabEnsureClient),
        });
        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Failed);
        assert!(
            result
                .steps
                .iter()
                .skip(4)
                .all(|step| step.status == WorkflowStatus::Skipped)
        );

        let runner = WorkflowRunner::new(WorkflowClients {
            git: Arc::new(FailingGitClient::new(WorkflowStepKind::MirrorPush)),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(MockGitLabClient),
        });
        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Failed);
        assert!(
            result
                .steps
                .iter()
                .skip(5)
                .all(|step| step.status == WorkflowStatus::Skipped)
        );

        let runner = WorkflowRunner::new(WorkflowClients {
            git: Arc::new(MockGitClient),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(FailingGitLabClient),
        });
        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Failed);
        assert!(
            result
                .steps
                .iter()
                .skip(6)
                .all(|step| step.status == WorkflowStatus::Skipped)
        );

        let runner = WorkflowRunner::new(WorkflowClients {
            git: Arc::new(MockGitClient),
            github: Arc::new(FailingGitHubClient),
            gitlab: Arc::new(MockGitLabClient),
        });
        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Failed);
        assert!(result.steps.last().unwrap().status == WorkflowStatus::Failed);
    }

    #[test]
    fn run_git_reports_errors() {
        let result = run_git(Path::new("/tmp/shipshape_missing"), &["status"], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn workflow_service_respects_mode() {
        let _guard = env_lock();
        let _live = WorkflowService::from_env();

        set_env("SHIPSHAPE_WORKFLOW_MODE", "mock");
        let service = WorkflowService::from_env();
        let (pool, _db) = test_pool();
        let result = service
            .run(&pool, "vessel-1", &sample_request())
            .expect("workflow run");
        assert_eq!(result.status, WorkflowStatus::Success);
        remove_env("SHIPSHAPE_WORKFLOW_MODE");
    }

    fn temp_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("shipshape_server_test_{nanos}"))
    }

    fn init_bare_repo() -> PathBuf {
        let root = temp_dir();
        std::fs::create_dir_all(&root).expect("create repo");
        Command::new("git")
            .args(["init", "--bare", "-q"])
            .current_dir(&root)
            .status()
            .expect("git init");
        root
    }

    fn init_work_repo(origin: &Path) -> PathBuf {
        let root = temp_dir();
        std::fs::create_dir_all(&root).expect("create repo");
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .expect("git init");
        Command::new("git")
            .args(["checkout", "-b", "main"])
            .current_dir(&root)
            .status()
            .expect("checkout main");
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
        Command::new("git")
            .args(["remote", "add", "origin", origin.to_str().unwrap()])
            .current_dir(&root)
            .status()
            .expect("git remote add");
        Command::new("git")
            .args(["push", "-u", "origin", "main"])
            .current_dir(&root)
            .status()
            .expect("git push");
        root
    }

    fn make_patch(repo: &Path) -> String {
        let readme = repo.join("README.md");
        std::fs::write(&readme, "shipshape\nrefit\n").expect("write readme");
        let output = Command::new("git")
            .args(["diff"])
            .current_dir(repo)
            .output()
            .expect("git diff");
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    fn set_env(key: &str, value: &str) {
        unsafe {
            std::env::set_var(key, value);
        }
    }

    fn remove_env(key: &str) {
        unsafe {
            std::env::remove_var(key);
        }
    }

    fn test_pool() -> (DbPool, crate::db::TestDatabase) {
        let mut test_db = crate::db::TestDatabase::new();
        let pool = test_db.pool();
        (pool, test_db)
    }

    #[derive(Clone)]
    struct FailingGitClient {
        fail_at: WorkflowStepKind,
    }

    impl FailingGitClient {
        fn new(fail_at: WorkflowStepKind) -> Self {
            Self { fail_at }
        }
    }

    impl GitClient for FailingGitClient {
        fn prepare_workspace(&self, _repo: &RepoSpec) -> Result<GitWorkspace, WorkflowError> {
            Ok(GitWorkspace {
                path: PathBuf::from("/tmp/shipshape-mock"),
                managed: false,
            })
        }

        fn apply_patch(
            &self,
            _workspace: &GitWorkspace,
            _patch: &PatchSpec,
        ) -> Result<String, WorkflowError> {
            Ok("commit".to_string())
        }

        fn create_branch(
            &self,
            _workspace: &GitWorkspace,
            _base_branch: &str,
            _branch: &str,
        ) -> Result<(), WorkflowError> {
            if self.fail_at == WorkflowStepKind::CreateBranch {
                return Err(WorkflowError::new("fail create"));
            }
            Ok(())
        }

        fn push_branch(
            &self,
            _workspace: &GitWorkspace,
            _repo: &RepoSpec,
            _branch: &str,
        ) -> Result<String, WorkflowError> {
            if self.fail_at == WorkflowStepKind::PushBranch {
                return Err(WorkflowError::new("fail push"));
            }
            Ok("origin/branch".to_string())
        }

        fn push_mirror(
            &self,
            _workspace: &GitWorkspace,
            _gitlab: &GitLabSpec,
            _branch: &str,
        ) -> Result<String, WorkflowError> {
            if self.fail_at == WorkflowStepKind::MirrorPush {
                return Err(WorkflowError::new("fail mirror"));
            }
            Ok("mirror/branch".to_string())
        }

        fn cleanup(&self, _workspace: GitWorkspace) -> Result<(), WorkflowError> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FailingGitHubClient;

    impl GitHubClient for FailingGitHubClient {
        fn open_pr(
            &self,
            _repo: &RepoSpec,
            _pr: &PullRequestSpec,
            _branch: &str,
        ) -> Result<String, WorkflowError> {
            Err(WorkflowError::new("fail pr"))
        }
    }

    #[derive(Clone)]
    struct FailingGitLabClient;

    impl GitLabClient for FailingGitLabClient {
        fn ensure_project(&self, _gitlab: &GitLabSpec) -> Result<(), WorkflowError> {
            Ok(())
        }

        fn trigger_pipeline(
            &self,
            _gitlab: &GitLabSpec,
            _branch: &str,
        ) -> Result<String, WorkflowError> {
            Err(WorkflowError::new("fail pipeline"))
        }
    }

    #[derive(Clone)]
    struct FailingGitLabEnsureClient;

    impl GitLabClient for FailingGitLabEnsureClient {
        fn ensure_project(&self, _gitlab: &GitLabSpec) -> Result<(), WorkflowError> {
            Err(WorkflowError::new("fail ensure"))
        }

        fn trigger_pipeline(
            &self,
            _gitlab: &GitLabSpec,
            _branch: &str,
        ) -> Result<String, WorkflowError> {
            Ok("pipeline".to_string())
        }
    }
}
