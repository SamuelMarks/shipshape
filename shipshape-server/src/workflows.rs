//! GitHub PR + GitLab mirror workflow orchestration.
//!
//! Coordinates workspace cloning, patch application, branch creation, GitHub push,
//! GitLab mirror creation/push, GitLab CI pipeline triggering, PR template
//! interpolation, and GitHub Pull Request creation, persisting workflow records
//! and step statuses to PostgreSQL.

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
use crate::error::{ServerError, ServerResult};
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
    /// GitLab project path (`namespace/project`).
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
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
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
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ApplyPatch => "apply_patch",
            Self::CreateBranch => "create_branch",
            Self::PushBranch => "push_branch",
            Self::EnsureGitlabProject => "ensure_gitlab_project",
            Self::MirrorPush => "mirror_push",
            Self::TriggerGitlab => "trigger_gitlab",
            Self::OpenPr => "open_pr",
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

/// Repository workspace used during git operations.
#[derive(Debug, Clone)]
pub struct GitWorkspace {
    /// Local directory path of the workspace.
    pub path: PathBuf,
    /// Whether the workspace directory was created by ShipShape and should be cleaned up.
    pub managed: bool,
}

/// Trait for git operations.
pub trait GitClient {
    /// Prepare a workspace for the repository.
    ///
    /// # Arguments
    /// * `repo` - Repository specification.
    fn prepare_workspace(&self, repo: &RepoSpec) -> ServerResult<GitWorkspace>;

    /// Apply a patch to the repository and return the resulting commit SHA.
    ///
    /// # Arguments
    /// * `workspace` - Target Git workspace.
    /// * `patch` - Patch specification.
    fn apply_patch(&self, workspace: &GitWorkspace, patch: &PatchSpec) -> ServerResult<String>;

    /// Create a new branch for the patch.
    ///
    /// # Arguments
    /// * `workspace` - Target Git workspace.
    /// * `base_branch` - Base branch name.
    /// * `branch` - New feature branch name.
    fn create_branch(
        &self,
        workspace: &GitWorkspace,
        base_branch: &str,
        branch: &str,
    ) -> ServerResult<()>;

    /// Push the branch to the primary remote.
    ///
    /// # Arguments
    /// * `workspace` - Target Git workspace.
    /// * `repo` - Repository specification.
    /// * `branch` - Feature branch name.
    fn push_branch(
        &self,
        workspace: &GitWorkspace,
        repo: &RepoSpec,
        branch: &str,
    ) -> ServerResult<String>;

    /// Push the branch to the GitLab mirror remote.
    ///
    /// # Arguments
    /// * `workspace` - Target Git workspace.
    /// * `gitlab` - GitLab mirror specification.
    /// * `branch` - Feature branch name.
    fn push_mirror(
        &self,
        workspace: &GitWorkspace,
        gitlab: &GitLabSpec,
        branch: &str,
    ) -> ServerResult<String>;

    /// Clean up any managed workspace resources.
    ///
    /// # Arguments
    /// * `workspace` - Workspace to clean up.
    fn cleanup(&self, workspace: GitWorkspace) -> ServerResult<()>;
}

/// Trait for GitHub API operations.
pub trait GitHubClient {
    /// Open a pull request for the branch.
    ///
    /// # Arguments
    /// * `repo` - Repository specification.
    /// * `pr` - Pull request specification.
    /// * `branch` - Head branch name.
    fn open_pr(&self, repo: &RepoSpec, pr: &PullRequestSpec, branch: &str) -> ServerResult<String>;
}

/// Trait for GitLab API operations.
pub trait GitLabClient {
    /// Ensure the GitLab project exists for mirroring.
    ///
    /// # Arguments
    /// * `gitlab` - GitLab mirror specification.
    fn ensure_project(&self, gitlab: &GitLabSpec) -> ServerResult<()>;

    /// Trigger a pipeline for the mirror.
    ///
    /// # Arguments
    /// * `gitlab` - GitLab mirror specification.
    /// * `branch` - Default branch reference if `gitlab.pipeline_ref` is `None`.
    fn trigger_pipeline(&self, gitlab: &GitLabSpec, branch: &str) -> ServerResult<String>;
}

/// Mock git client used for local testing.
#[derive(Debug, Default, Clone)]
pub struct MockGitClient;

impl GitClient for MockGitClient {
    fn prepare_workspace(&self, repo: &RepoSpec) -> ServerResult<GitWorkspace> {
        if repo.repo_url.trim().is_empty() {
            return Err(ServerError::workflow("repo_url is required"));
        }
        if repo.base_branch.trim().is_empty() {
            return Err(ServerError::workflow("base_branch is required"));
        }
        Ok(GitWorkspace {
            path: PathBuf::from("/tmp/shipshape-mock"),
            managed: false,
        })
    }

    fn apply_patch(&self, _workspace: &GitWorkspace, patch: &PatchSpec) -> ServerResult<String> {
        if patch.diff.trim().is_empty() {
            return Err(ServerError::workflow("patch diff is empty"));
        }
        Ok("mock-commit-sha".to_string())
    }

    fn create_branch(
        &self,
        _workspace: &GitWorkspace,
        base_branch: &str,
        branch: &str,
    ) -> ServerResult<()> {
        if base_branch.trim().is_empty() {
            return Err(ServerError::workflow("base branch is required"));
        }
        if branch.trim().is_empty() {
            return Err(ServerError::workflow("branch name is required"));
        }
        Ok(())
    }

    fn push_branch(
        &self,
        _workspace: &GitWorkspace,
        repo: &RepoSpec,
        branch: &str,
    ) -> ServerResult<String> {
        if repo.repo_url.trim().is_empty() {
            return Err(ServerError::workflow("repo_url is required"));
        }
        Ok(format!("origin/{branch}"))
    }

    fn push_mirror(
        &self,
        _workspace: &GitWorkspace,
        gitlab: &GitLabSpec,
        branch: &str,
    ) -> ServerResult<String> {
        if gitlab.mirror_url.trim().is_empty() {
            return Err(ServerError::workflow("gitlab mirror_url is required"));
        }
        Ok(format!("mirror/{branch}"))
    }

    fn cleanup(&self, _workspace: GitWorkspace) -> ServerResult<()> {
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
    ) -> ServerResult<String> {
        if repo.repo_url.trim().is_empty() {
            return Err(ServerError::workflow("repo_url is required"));
        }
        if pr.title.trim().is_empty() {
            return Err(ServerError::workflow("pull request title is required"));
        }
        Ok("https://github.com/shipshape/mock/pull/1".to_string())
    }
}

/// Mock GitLab client used for local testing.
#[derive(Debug, Default, Clone)]
pub struct MockGitLabClient;

impl GitLabClient for MockGitLabClient {
    fn ensure_project(&self, gitlab: &GitLabSpec) -> ServerResult<()> {
        if gitlab.project_path.trim().is_empty() {
            return Err(ServerError::workflow("gitlab project_path is required"));
        }
        Ok(())
    }

    fn trigger_pipeline(&self, gitlab: &GitLabSpec, branch: &str) -> ServerResult<String> {
        if gitlab.project_path.trim().is_empty() {
            return Err(ServerError::workflow("gitlab project_path is required"));
        }
        let reference = gitlab.pipeline_ref.as_deref().unwrap_or(branch);
        Ok(format!(
            "https://gitlab.example.com/{}/-/pipelines/{}",
            gitlab.project_path, reference
        ))
    }
}

/// Git client implemented with `git` shell commands.
#[derive(Debug, Clone)]
pub struct GitCommandClient {
    /// Root directory under which temporary repository workspaces are created.
    workspace_root: PathBuf,
    /// Whether to preserve workspaces on disk after workflow completion.
    keep_workspace: bool,
    /// Optional GitHub token used for authenticated HTTPS cloning/pushing.
    github_token: Option<String>,
    /// Optional GitLab token used for authenticated HTTPS mirror pushing.
    gitlab_token: Option<String>,
    /// Git commit author name.
    author_name: String,
    /// Git commit author email.
    author_email: String,
}

impl GitCommandClient {
    /// Build a git command client from environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        let workspace_root = match std::env::var("SHIPSHAPE_WORKSPACE_ROOT") {
            Ok(val) => PathBuf::from(val),
            Err(_) => std::env::temp_dir().join("shipshape-workflows"),
        };
        let keep_workspace = match std::env::var("SHIPSHAPE_KEEP_WORKSPACE") {
            Ok(value) => value == "1" || value.eq_ignore_ascii_case("true"),
            Err(_) => false,
        };
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
    fn prepare_workspace(&self, repo: &RepoSpec) -> ServerResult<GitWorkspace> {
        if repo.base_branch.trim().is_empty() {
            return Err(ServerError::workflow("base_branch is required"));
        }
        if let Some(local_path) = repo.local_path.as_ref() {
            let path = PathBuf::from(local_path);
            if !path.is_dir() {
                return Err(ServerError::workflow(format!(
                    "local_path not found: {local_path}"
                )));
            }
            if !path.join(".git").is_dir() {
                return Err(ServerError::workflow("local_path is not a git repository"));
            }
            return Ok(GitWorkspace {
                path,
                managed: false,
            });
        }

        if repo.repo_url.trim().is_empty() {
            return Err(ServerError::workflow("repo_url is required"));
        }

        std::fs::create_dir_all(&self.workspace_root)?;
        let workspace_path = self.workspace_root.join(Uuid::new_v4().to_string());
        let workspace_str = workspace_path.to_string_lossy();
        let clone_url = maybe_inject_token(
            &repo.repo_url,
            self.github_token.as_deref(),
            "x-access-token",
        );
        let clone_args = ["clone", &clone_url, workspace_str.as_ref()];
        run_git(&self.workspace_root, &clone_args, &[])?;
        let fetch_args = ["fetch", "origin", &repo.base_branch];
        run_git(&workspace_path, &fetch_args, &[])?;
        run_git(&workspace_path, &["checkout", &repo.base_branch], &[])?;

        Ok(GitWorkspace {
            path: workspace_path,
            managed: true,
        })
    }

    fn apply_patch(&self, workspace: &GitWorkspace, patch: &PatchSpec) -> ServerResult<String> {
        if patch.diff.trim().is_empty() {
            return Err(ServerError::workflow("patch diff is empty"));
        }

        let patch_path = workspace.path.join("shipshape.patch");
        std::fs::write(&patch_path, patch.diff.as_bytes())?;
        let patch_str = patch_path.to_string_lossy();
        run_git(&workspace.path, &["apply", patch_str.as_ref()], &[])?;

        let status = run_git(&workspace.path, &["status", "--porcelain"], &[])?;
        let _ = status.trim();

        run_git(&workspace.path, &["add", "-A"], &[])?;
        let envs = [
            ("GIT_AUTHOR_NAME", self.author_name.as_str()),
            ("GIT_AUTHOR_EMAIL", self.author_email.as_str()),
            ("GIT_COMMITTER_NAME", self.author_name.as_str()),
            ("GIT_COMMITTER_EMAIL", self.author_email.as_str()),
        ];
        let commit_args = ["commit", "-m", patch.commit_message.as_str()];
        run_git(&workspace.path, &commit_args, &envs)?;

        let commit = run_git(&workspace.path, &["rev-parse", "HEAD"], &[])?;
        Ok(commit.trim().to_string())
    }

    fn create_branch(
        &self,
        workspace: &GitWorkspace,
        base_branch: &str,
        branch: &str,
    ) -> ServerResult<()> {
        if branch.trim().is_empty() {
            return Err(ServerError::workflow("branch name is required"));
        }
        run_git(&workspace.path, &["checkout", base_branch], &[])?;
        let branch_args = ["checkout", "-B", branch, base_branch];
        run_git(&workspace.path, &branch_args, &[])?;
        Ok(())
    }

    fn push_branch(
        &self,
        workspace: &GitWorkspace,
        repo: &RepoSpec,
        branch: &str,
    ) -> ServerResult<String> {
        let remote_url = maybe_inject_token(
            &repo.repo_url,
            self.github_token.as_deref(),
            "x-access-token",
        );
        let set_url_args = ["remote", "set-url", "origin", remote_url.as_str()];
        run_git(&workspace.path, &set_url_args, &[])?;
        run_git(&workspace.path, &["push", "origin", branch], &[])?;
        Ok(format!("origin/{branch}"))
    }

    fn push_mirror(
        &self,
        workspace: &GitWorkspace,
        gitlab: &GitLabSpec,
        branch: &str,
    ) -> ServerResult<String> {
        if gitlab.mirror_url.trim().is_empty() {
            return Err(ServerError::workflow("gitlab mirror_url is required"));
        }
        let mirror_url =
            maybe_inject_token(&gitlab.mirror_url, self.gitlab_token.as_deref(), "oauth2");
        let set_args = ["remote", "set-url", "shipshape-mirror", mirror_url.as_str()];
        let add_args = ["remote", "add", "shipshape-mirror", mirror_url.as_str()];
        if run_git(&workspace.path, &set_args, &[]).is_err() {
            run_git(&workspace.path, &add_args, &[])?;
        }
        let push_args = ["push", "shipshape-mirror", branch];
        run_git(&workspace.path, &push_args, &[])?;
        Ok(format!("mirror/{branch}"))
    }

    fn cleanup(&self, workspace: GitWorkspace) -> ServerResult<()> {
        if workspace.managed && !self.keep_workspace {
            std::fs::remove_dir_all(&workspace.path)?;
        }
        Ok(())
    }
}

/// GitHub API client implementation.
#[derive(Debug, Clone)]
pub struct GitHubApiClient {
    /// GitHub API base URL.
    base_url: String,
    /// Optional personal access or OAuth token.
    token: Option<String>,
    /// HTTP client.
    client: Client,
    /// HTTP `User-Agent` header value.
    user_agent: String,
}

impl GitHubApiClient {
    /// Build a GitHub API client from environment variables.
    #[must_use]
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
    fn open_pr(&self, repo: &RepoSpec, pr: &PullRequestSpec, branch: &str) -> ServerResult<String> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| ServerError::workflow("GITHUB_TOKEN is required"))?;
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
            .send()?;
        if !response.status().is_success() {
            let status = response.status().to_string();
            let detail = response.text().unwrap_or_default();
            return Err(ServerError::GitHubApi { status, detail });
        }
        let value: serde_json::Value = response.json()?;
        let html_url = value
            .get("html_url")
            .and_then(|val| val.as_str())
            .ok_or_else(|| ServerError::workflow("github response missing html_url"))?;
        Ok(html_url.to_string())
    }
}

/// GitLab API client implementation.
#[derive(Debug, Clone)]
pub struct GitLabApiClient {
    /// GitLab instance base URL.
    base_url: String,
    /// Optional GitLab personal/project access token.
    token: Option<String>,
    /// HTTP client.
    client: Client,
}

/// GitLab namespace lookup entry.
#[derive(Debug, Deserialize)]
struct GitLabNamespace {
    /// Numeric namespace ID.
    id: i64,
    /// Full namespace path (`group/subgroup`).
    full_path: String,
}

impl GitLabApiClient {
    /// Build a GitLab API client from environment variables.
    #[must_use]
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

    /// Resolve the numeric GitLab namespace ID for `namespace_path`.
    ///
    /// # Arguments
    /// * `token` - GitLab private token.
    /// * `namespace_path` - Full namespace path to search for.
    fn resolve_namespace_id(&self, token: &str, namespace_path: &str) -> ServerResult<i64> {
        let url = format!("{}/api/v4/namespaces", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .get(url)
            .header("PRIVATE-TOKEN", token)
            .query(&[("search", namespace_path)])
            .send()?;
        if !response.status().is_success() {
            let status = response.status().to_string();
            let detail = response.text().unwrap_or_default();
            return Err(ServerError::GitLabApi { status, detail });
        }
        let namespaces: Vec<GitLabNamespace> = response.json()?;
        let namespace = namespaces
            .into_iter()
            .find(|candidate| candidate.full_path == namespace_path)
            .ok_or_else(|| {
                ServerError::workflow(format!("gitlab namespace not found: {namespace_path}"))
            })?;
        Ok(namespace.id)
    }
}

impl GitLabClient for GitLabApiClient {
    fn ensure_project(&self, gitlab: &GitLabSpec) -> ServerResult<()> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| ServerError::workflow("GITLAB_TOKEN is required"))?;
        let trimmed_path = gitlab.project_path.trim();
        if trimmed_path.is_empty() {
            return Err(ServerError::workflow("gitlab project_path is required"));
        }
        let base = self.base_url.trim_end_matches('/');
        let encoded = urlencoding::encode(trimmed_path);
        let url = format!("{base}/api/v4/projects/{encoded}");
        let response = self.client.get(url).header("PRIVATE-TOKEN", token).send()?;
        if response.status().is_success() {
            return Ok(());
        }
        if response.status().as_u16() != 404 {
            let status = response.status().to_string();
            let detail = response.text().unwrap_or_default();
            return Err(ServerError::GitLabApi { status, detail });
        }

        let (namespace_path, project_name) = split_gitlab_project_path(trimmed_path)?;
        let mut form = vec![
            ("name", project_name.clone()),
            ("path", project_name.clone()),
        ];
        if let Some(namespace_path) = namespace_path {
            let namespace_id = self.resolve_namespace_id(token, &namespace_path)?;
            form.push(("namespace_id", namespace_id.to_string()));
        }

        let url = format!("{base}/api/v4/projects");
        let req = self.client.post(url).header("PRIVATE-TOKEN", token);
        let response = req.form(&form).send()?;
        if response.status().is_success() || response.status().as_u16() == 409 {
            return Ok(());
        }
        let status = response.status().to_string();
        let detail = response.text().unwrap_or_default();
        Err(ServerError::GitLabApi { status, detail })
    }

    fn trigger_pipeline(&self, gitlab: &GitLabSpec, branch: &str) -> ServerResult<String> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| ServerError::workflow("GITLAB_TOKEN is required"))?;
        let trimmed_path = gitlab.project_path.trim();
        if trimmed_path.is_empty() {
            return Err(ServerError::workflow("gitlab project_path is required"));
        }
        let reference = gitlab.pipeline_ref.as_deref().unwrap_or(branch);
        let encoded = urlencoding::encode(trimmed_path);
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
            .send()?;
        if !response.status().is_success() {
            let status = response.status().to_string();
            let detail = response.text().unwrap_or_default();
            return Err(ServerError::GitLabApi { status, detail });
        }
        let value: serde_json::Value = response.json()?;
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
                    trimmed_path
                ));
            }
            return Err(ServerError::workflow("gitlab response missing web_url"));
        }
        Ok(pipeline_url.to_string())
    }
}

/// Shared workflow client set.
#[derive(Clone)]
pub struct WorkflowClients {
    /// Git subprocess client.
    git: Arc<dyn GitClient + Send + Sync>,
    /// GitHub API client.
    github: Arc<dyn GitHubClient + Send + Sync>,
    /// GitLab API client.
    gitlab: Arc<dyn GitLabClient + Send + Sync>,
}

impl WorkflowClients {
    /// Build mock workflow clients.
    #[must_use]
    pub fn mock() -> Self {
        Self {
            git: Arc::new(MockGitClient),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(MockGitLabClient),
        }
    }

    /// Build live workflow clients from environment configuration.
    #[must_use]
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
    /// Underlying Git, GitHub, and GitLab clients.
    clients: WorkflowClients,
}

impl WorkflowService {
    /// Build a workflow service with mock clients.
    #[must_use]
    pub fn mock() -> Self {
        Self {
            clients: WorkflowClients::mock(),
        }
    }

    /// Build a workflow service from environment configuration.
    #[must_use]
    pub fn from_env() -> Self {
        let mode = std::env::var("SHIPSHAPE_WORKFLOW_MODE").unwrap_or_else(|_| "live".to_string());
        if mode.eq_ignore_ascii_case("mock") {
            return Self::mock();
        }
        Self {
            clients: WorkflowClients::from_env(),
        }
    }

    /// Run the workflow and persist results to PostgreSQL.
    ///
    /// # Arguments
    /// * `pool` - Database connection pool.
    /// * `vessel_id` - Target vessel identifier.
    /// * `request` - Workflow execution parameters.
    pub fn run(
        &self,
        pool: &DbPool,
        vessel_id: &str,
        request: &WorkflowRequest,
    ) -> ServerResult<WorkflowResult> {
        let runner = WorkflowRunner::new(self.clients.clone());
        let result = runner.run(request);
        persist_workflow(pool, vessel_id, &result)?;
        Ok(result)
    }
}

/// Orchestrates the GitHub PR + GitLab mirror workflow.
#[derive(Clone)]
pub struct WorkflowRunner {
    /// Injected Git, GitHub, and GitLab clients.
    clients: WorkflowClients,
}

impl WorkflowRunner {
    /// Build a workflow runner with mock clients.
    #[cfg(test)]
    pub fn mock() -> Self {
        Self::new(WorkflowClients::mock())
    }

    /// Build a workflow runner with explicit clients.
    ///
    /// # Arguments
    /// * `clients` - Configured workflow clients.
    #[must_use]
    pub fn new(clients: WorkflowClients) -> Self {
        Self { clients }
    }

    /// Execute the workflow with the configured clients.
    ///
    /// # Arguments
    /// * `request` - Workflow request parameters.
    #[must_use]
    pub fn run(&self, request: &WorkflowRequest) -> WorkflowResult {
        let workflow_id = Uuid::new_v4().to_string();
        let mut steps = Vec::new();
        let mut overall_status = WorkflowStatus::Success;
        let mut pr_url = None;

        let workspace = match self.clients.git.prepare_workspace(&request.repo) {
            Ok(workspace) => workspace,
            Err(err) => {
                return abort_workflow(
                    workflow_id,
                    steps,
                    WorkflowStepKind::ApplyPatch,
                    err.to_string(),
                );
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
                let _ = self.clients.git.cleanup(workspace);
                return abort_workflow(
                    workflow_id,
                    steps,
                    WorkflowStepKind::ApplyPatch,
                    err.to_string(),
                );
            }
        };

        if let Err(err) = self.clients.git.create_branch(
            &workspace,
            &request.repo.base_branch,
            &request.patch.branch,
        ) {
            let _ = self.clients.git.cleanup(workspace);
            return abort_workflow(
                workflow_id,
                steps,
                WorkflowStepKind::CreateBranch,
                err.to_string(),
            );
        }
        steps.push(step_success(
            WorkflowStepKind::CreateBranch,
            Some(format!(
                "created branch {} from {}",
                request.patch.branch, request.repo.base_branch
            )),
        ));

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
                let _ = self.clients.git.cleanup(workspace);
                return abort_workflow(
                    workflow_id,
                    steps,
                    WorkflowStepKind::PushBranch,
                    err.to_string(),
                );
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
                let _ = self.clients.git.cleanup(workspace);
                return abort_workflow(
                    workflow_id,
                    steps,
                    WorkflowStepKind::EnsureGitlabProject,
                    err.to_string(),
                );
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
                let _ = self.clients.git.cleanup(workspace);
                return abort_workflow(
                    workflow_id,
                    steps,
                    WorkflowStepKind::MirrorPush,
                    err.to_string(),
                );
            }
        }

        let pipeline_url = match self
            .clients
            .gitlab
            .trigger_pipeline(&request.gitlab, &request.patch.branch)
        {
            Ok(url) => {
                steps.push(step_success(
                    WorkflowStepKind::TriggerGitlab,
                    Some(format!("triggered pipeline {url}")),
                ));
                Some(url)
            }
            Err(err) => {
                let _ = self.clients.git.cleanup(workspace);
                return abort_workflow(
                    workflow_id,
                    steps,
                    WorkflowStepKind::TriggerGitlab,
                    err.to_string(),
                );
            }
        };

        let pr_spec = match build_pr_spec(request, &workspace) {
            Ok(pr_spec) => pr_spec,
            Err(err) => {
                steps.push(step_failed(WorkflowStepKind::OpenPr, err.to_string()));
                let _ = self.clients.git.cleanup(workspace);
                return WorkflowResult {
                    workflow_id,
                    status: WorkflowStatus::Failed,
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

/// Record a failed step, append skipped downstream steps, and return a failed [`WorkflowResult`].
///
/// # Arguments
/// * `workflow_id` - Generated workflow UUID.
/// * `steps` - Steps accumulated prior to the failure.
/// * `failed_kind` - Step kind that failed.
/// * `detail` - Error detail message.
fn abort_workflow(
    workflow_id: String,
    mut steps: Vec<WorkflowStep>,
    failed_kind: WorkflowStepKind,
    detail: String,
) -> WorkflowResult {
    steps.push(step_failed(failed_kind, detail));
    push_skipped_steps(&mut steps, next_steps(failed_kind));
    WorkflowResult {
        workflow_id,
        status: WorkflowStatus::Failed,
        steps,
        pr_url: None,
        pipeline_url: None,
    }
}

/// Build the pull request specification, interpolating any PR template found in `workspace`.
///
/// # Arguments
/// * `request` - Workflow request containing PR metadata and optional [`FleetReport`].
/// * `workspace` - Local repository workspace.
fn build_pr_spec(
    request: &WorkflowRequest,
    workspace: &GitWorkspace,
) -> ServerResult<PullRequestSpec> {
    let mut pr = request.pr.clone();
    let Some(report) = request.fleet_report.as_ref() else {
        return Ok(pr);
    };
    let fs = StdFileSystem::new();
    let context = PrTemplateContext::from_report(report);
    if let Some(rendered) = interpolate_pr_template(&fs, &workspace.path, &context)? {
        let extra = pr.body.as_deref();
        pr.body = Some(merge_pr_body(rendered, extra));
    }
    Ok(pr)
}

/// Merge an interpolated PR template with optional extra PR body text.
///
/// # Arguments
/// * `template` - Rendered PR template text.
/// * `extra` - Optional extra notes from the request.
fn merge_pr_body(template: String, extra: Option<&str>) -> String {
    let extra = extra.map(str::trim).filter(|value| !value.is_empty());
    match extra {
        Some(extra) => format!(
            "{template}

{extra}"
        ),
        None => template,
    }
}

/// Persist the workflow record and its steps into PostgreSQL.
///
/// # Arguments
/// * `pool` - Database connection pool.
/// * `vessel_id` - Vessel identifier.
/// * `result` - Completed workflow result.
fn persist_workflow(pool: &DbPool, vessel_id: &str, result: &WorkflowResult) -> ServerResult<()> {
    let mut conn = pool.get()?;
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
        .execute(&mut conn)?;

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
    diesel::insert_into(workflow_steps::table)
        .values(&steps)
        .execute(&mut conn)?;

    Ok(())
}

/// Construct a successful [`WorkflowStep`].
///
/// # Arguments
/// * `kind` - Step kind.
/// * `detail` - Optional detail summary.
fn step_success(kind: WorkflowStepKind, detail: Option<String>) -> WorkflowStep {
    WorkflowStep {
        kind,
        status: WorkflowStatus::Success,
        detail,
    }
}

/// Construct a failed [`WorkflowStep`].
///
/// # Arguments
/// * `kind` - Step kind.
/// * `detail` - Error detail message.
fn step_failed(kind: WorkflowStepKind, detail: String) -> WorkflowStep {
    WorkflowStep {
        kind,
        status: WorkflowStatus::Failed,
        detail: Some(detail),
    }
}

/// Construct a skipped [`WorkflowStep`].
///
/// # Arguments
/// * `kind` - Step kind.
fn step_skipped(kind: WorkflowStepKind) -> WorkflowStep {
    WorkflowStep {
        kind,
        status: WorkflowStatus::Skipped,
        detail: Some("skipped due to prior failure".to_string()),
    }
}

/// Return the slice of downstream steps following `start`.
///
/// # Arguments
/// * `start` - Step kind that failed.
const fn next_steps(start: WorkflowStepKind) -> &'static [WorkflowStepKind] {
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
        WorkflowStepKind::TriggerGitlab | WorkflowStepKind::OpenPr => &[WorkflowStepKind::OpenPr],
    }
}

/// Append skipped steps for each item in `remaining`.
///
/// # Arguments
/// * `steps` - Step vector to append to.
/// * `remaining` - Downstream step kinds to mark skipped.
fn push_skipped_steps(steps: &mut Vec<WorkflowStep>, remaining: &[WorkflowStepKind]) {
    for kind in remaining {
        steps.push(step_skipped(*kind));
    }
}

/// Extract `(owner, repo)` from a GitHub HTTPS, HTTP, or SSH URL.
///
/// # Arguments
/// * `url` - GitHub repository URL.
fn parse_github_repo(url: &str) -> ServerResult<(String, String)> {
    let trimmed = url.trim().trim_end_matches(".git");
    let rest = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("git@github.com:"))
        .ok_or_else(|| ServerError::workflow("unsupported github repo url"))?;
    let Some((owner, repo)) = rest.split_once('/') else {
        return Err(ServerError::workflow("missing github repo"));
    };
    if owner.is_empty() || repo.is_empty() {
        return Err(ServerError::workflow("missing github owner or repo"));
    }
    Ok((owner.to_string(), repo.to_string()))
}

/// Split a GitLab `project_path` into an optional namespace path and project name.
///
/// # Arguments
/// * `project_path` - GitLab project path (`namespace/project` or `project`).
fn split_gitlab_project_path(project_path: &str) -> ServerResult<(Option<String>, String)> {
    let trimmed = project_path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(ServerError::workflow("gitlab project_path is required"));
    }
    match trimmed.rsplit_once('/') {
        Some((namespace, name)) => {
            let namespace = namespace.trim().trim_end_matches('/');
            let name = name.trim();
            if name.is_empty() {
                return Err(ServerError::workflow(
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

/// Inject `username:token@` into an `https://` Git remote URL if not already present.
///
/// # Arguments
/// * `url` - Remote repository URL.
/// * `token` - Optional secret access token.
/// * `username` - Auth username prefix (`"x-access-token"` or `"oauth2"`).
fn maybe_inject_token(url: &str, token: Option<&str>, username: &str) -> String {
    let Some(token) = token else {
        return url.to_string();
    };
    let Some(rest) = url.strip_prefix("https://") else {
        return url.to_string();
    };
    if rest.contains('@') {
        return url.to_string();
    }
    format!("https://{username}:{token}@{rest}")
}

/// Redact any `https://user:token@` credentials embedded in `text`.
///
/// # Arguments
/// * `text` - String that may contain an authenticated HTTPS URL.
fn redact_credentials(text: &str) -> String {
    let Some(https_idx) = text.find("https://") else {
        return text.to_string();
    };
    let after_scheme = &text[https_idx + "https://".len()..];
    let authority_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let Some(at_idx) = authority.find('@') else {
        return text.to_string();
    };
    let prefix = &text[..https_idx + "https://".len()];
    let suffix = &after_scheme[at_idx + 1..];
    format!("{prefix}***@{suffix}")
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

/// Execute a `git` subprocess in `path` with `args` and `envs`, redacting credentials on failure.
///
/// # Arguments
/// * `path` - Working directory for `git`.
/// * `args` - CLI arguments passed to `git`.
/// * `envs` - Environment variable overrides.
fn run_git(path: &Path, args: &[&str], envs: &[(&str, &str)]) -> ServerResult<String> {
    let mut command = Command::new("git");
    command.args(args).current_dir(path);
    for key in GIT_LOCAL_ENV_VARS {
        command.env_remove(key);
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        let raw_detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        let sanitized_args: Vec<String> = args.iter().map(|a| redact_credentials(a)).collect();
        let detail = redact_credentials(raw_detail);
        return Err(ServerError::GitCommandFailed {
            args: sanitized_args,
            detail,
        });
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
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counter for unique test directories.
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Build a sample [`WorkflowRequest`] for unit tests.
    fn sample_request() -> WorkflowRequest {
        WorkflowRequest {
            repo: RepoSpec {
                repo_url: "https://github.com/shipshape/shipshape-demo.git".to_string(),
                base_branch: "main".to_string(),
                local_path: None,
            },
            patch: PatchSpec {
                diff: "diff --git a/README.md b/README.md
"
                .to_string(),
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
    fn workflow_fails_when_workspace_or_patch_missing() {
        let runner = WorkflowRunner::mock();
        let mut bad_repo_req = sample_request();
        bad_repo_req.repo.repo_url = String::new();
        let res_repo = runner.run(&bad_repo_req);
        assert_eq!(res_repo.status, WorkflowStatus::Failed);
        assert_eq!(res_repo.steps.len(), 7);

        let mut request = sample_request();
        request.patch.diff = String::new();
        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Failed);
        assert_eq!(result.steps[0].status, WorkflowStatus::Failed);
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
    fn parse_github_repo_accepts_common_forms_and_rejects_invalid() -> ServerResult<()> {
        let (owner, repo) = parse_github_repo("https://github.com/shipshape/demo.git")?;
        assert_eq!(owner, "shipshape");
        assert_eq!(repo, "demo");

        let (owner, repo) = parse_github_repo("http://github.com/shipshape/demo")?;
        assert_eq!(owner, "shipshape");
        assert_eq!(repo, "demo");

        let (owner, repo) = parse_github_repo("git@github.com:shipshape/demo.git")?;
        assert_eq!(owner, "shipshape");
        assert_eq!(repo, "demo");

        assert!(parse_github_repo("https://example.com/other").is_err());
        assert!(parse_github_repo("https://github.com/onlyowner").is_err());
        assert!(parse_github_repo("https://github.com//repo").is_err());
        assert!(parse_github_repo("https://github.com/owner/.git").is_err());
        Ok(())
    }

    #[test]
    fn split_gitlab_project_path_parses_variants() -> ServerResult<()> {
        let (namespace, name) = split_gitlab_project_path("demo")?;
        assert!(namespace.is_none());
        assert_eq!(name, "demo");

        let (namespace, name) = split_gitlab_project_path("shipshape/demo")?;
        assert_eq!(namespace.as_deref(), Some("shipshape"));
        assert_eq!(name, "demo");

        let (namespace, name) = split_gitlab_project_path("///demo")?;
        assert!(namespace.is_none());
        assert_eq!(name, "demo");

        let (namespace, name) = split_gitlab_project_path(" / /demo")?;
        assert!(namespace.is_none());
        assert_eq!(name, "demo");

        assert!(split_gitlab_project_path("   ").is_err());
        assert!(split_gitlab_project_path("shipshape/").is_err());
        Ok(())
    }

    #[test]
    fn maybe_inject_token_and_redact_credentials_behave_safely() {
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
        let injected = maybe_inject_token("https://github.com/org/repo", Some("secret123"), "x");
        assert_eq!(injected, "https://x:secret123@github.com/org/repo");

        let redacted = redact_credentials(&injected);
        assert_eq!(redacted, "https://***@github.com/org/repo");
        assert!(!redacted.contains("secret123"));
        assert_eq!(
            redact_credentials("https://github.com/org/repo"),
            "https://github.com/org/repo"
        );
        assert_eq!(redact_credentials("plain error"), "plain error");
    }

    #[test]
    fn mock_clients_validate_all_inputs() -> ServerResult<()> {
        let git = MockGitClient;
        let ws = GitWorkspace {
            path: PathBuf::from("/tmp"),
            managed: false,
        };
        let empty_repo = RepoSpec {
            repo_url: String::new(),
            base_branch: String::new(),
            local_path: None,
        };
        assert!(git.prepare_workspace(&empty_repo).is_err());

        let missing_base = RepoSpec {
            repo_url: "https://github.com/org/repo.git".to_string(),
            base_branch: String::new(),
            local_path: None,
        };
        assert!(git.prepare_workspace(&missing_base).is_err());
        assert!(git.create_branch(&ws, "", "feat").is_err());
        assert!(git.create_branch(&ws, "main", "").is_err());
        assert!(git.push_branch(&ws, &empty_repo, "feat").is_err());

        let empty_gitlab = GitLabSpec {
            mirror_url: String::new(),
            project_path: String::new(),
            pipeline_ref: None,
        };
        assert!(git.push_mirror(&ws, &empty_gitlab, "feat").is_err());

        let github = MockGitHubClient;
        let pr = PullRequestSpec {
            title: String::new(),
            body: None,
            draft: false,
        };
        assert!(github.open_pr(&empty_repo, &pr, "branch").is_err());
        assert!(github.open_pr(&missing_base, &pr, "branch").is_err());

        let gitlab = MockGitLabClient;
        assert!(gitlab.ensure_project(&empty_gitlab).is_err());
        assert!(gitlab.trigger_pipeline(&empty_gitlab, "branch").is_err());
        Ok(())
    }

    #[test]
    fn workflow_interpolates_pr_template_and_handles_missing_or_unreadable_template()
    -> ServerResult<()> {
        let root = temp_dir();
        std::fs::create_dir_all(&root)?;
        let template_file = root.join("PULL_REQUEST_TEMPLATE.md");
        let tpl = format!(
            "{}
{}
{}
",
            shipshape_core::SHIPSHAPE_STATS,
            shipshape_core::SHIPSHAPE_FIXES,
            shipshape_core::SHIPSHAPE_CI
        );
        std::fs::write(&template_file, tpl)?;

        let git = Arc::new(TemplateGitClient { root: root.clone() });
        let github = Arc::new(RecordingGitHubClient::default());
        let runner = WorkflowRunner::new(WorkflowClients {
            git,
            github: Arc::clone(&github) as Arc<dyn GitHubClient + Send + Sync>,
            gitlab: Arc::new(MockGitLabClient),
        });

        let mut request = sample_request();
        request.fleet_report = Some(sample_report());
        request.pr.body = Some("Extra notes.".to_string());

        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Success);
        let body = github.last_body().unwrap_or_default();
        assert!(body.contains("ShipShape stats:"));
        assert!(body.contains("Extra notes."));

        // Without extra body
        request.pr.body = None;
        let result_no_extra = runner.run(&request);
        assert_eq!(result_no_extra.status, WorkflowStatus::Success);

        // Without template file
        std::fs::remove_file(&template_file)?;
        request.pr.body = Some("Original body.".to_string());
        let result_no_tpl = runner.run(&request);
        assert_eq!(result_no_tpl.status, WorkflowStatus::Success);
        assert_eq!(github.last_body().as_deref(), Some("Original body."));

        // Invalid UTF-8 in template triggers build_pr_spec error branch
        std::fs::write(&template_file, [0xff, 0xfe, 0xfd])?;
        let result_err_tpl = runner.run(&request);
        assert_eq!(result_err_tpl.status, WorkflowStatus::Failed);
        assert_eq!(result_err_tpl.steps.len(), 7);
        assert_eq!(result_err_tpl.steps[6].status, WorkflowStatus::Failed);

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// GitClient stub backed by a fixed local directory for PR template tests.
    #[derive(Debug)]
    struct TemplateGitClient {
        /// Workspace root directory.
        root: PathBuf,
    }

    impl GitClient for TemplateGitClient {
        fn prepare_workspace(&self, _repo: &RepoSpec) -> ServerResult<GitWorkspace> {
            Ok(GitWorkspace {
                path: self.root.clone(),
                managed: false,
            })
        }

        fn apply_patch(
            &self,
            _workspace: &GitWorkspace,
            _patch: &PatchSpec,
        ) -> ServerResult<String> {
            Ok("mock-commit-sha".to_string())
        }

        fn create_branch(
            &self,
            _workspace: &GitWorkspace,
            _base_branch: &str,
            _branch: &str,
        ) -> ServerResult<()> {
            Ok(())
        }

        fn push_branch(
            &self,
            _workspace: &GitWorkspace,
            _repo: &RepoSpec,
            branch: &str,
        ) -> ServerResult<String> {
            Ok(format!("origin/{branch}"))
        }

        fn push_mirror(
            &self,
            _workspace: &GitWorkspace,
            _gitlab: &GitLabSpec,
            branch: &str,
        ) -> ServerResult<String> {
            Ok(format!("mirror/{branch}"))
        }

        fn cleanup(&self, _workspace: GitWorkspace) -> ServerResult<()> {
            Ok(())
        }
    }

    /// GitHubClient stub that records the last PR body passed to `open_pr`.
    #[derive(Debug, Default)]
    struct RecordingGitHubClient {
        /// Recorded PR body.
        last_body: Mutex<Option<String>>,
    }

    impl RecordingGitHubClient {
        /// Return the recorded PR body.
        fn last_body(&self) -> Option<String> {
            self.last_body.lock().ok().and_then(|g| g.clone())
        }
    }

    impl GitHubClient for RecordingGitHubClient {
        fn open_pr(
            &self,
            _repo: &RepoSpec,
            pr: &PullRequestSpec,
            _branch: &str,
        ) -> ServerResult<String> {
            let _ = self.last_body.lock().map(|mut g| *g = pr.body.clone());
            Ok("https://github.com/shipshape/mock/pull/1".to_string())
        }
    }

    /// Build a sample [`FleetReport`] for PR template tests.
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

    #[test]
    fn git_command_client_validates_local_paths_and_runs_full_flow() -> ServerResult<()> {
        let _guard = env_lock()?;
        set_env("SHIPSHAPE_KEEP_WORKSPACE", "1");
        let _keep_client = GitCommandClient::from_env();
        set_env("SHIPSHAPE_KEEP_WORKSPACE", "true");
        let keep_true_client = GitCommandClient::from_env();
        keep_true_client.cleanup(GitWorkspace {
            path: PathBuf::from("/tmp/shipshape-kept"),
            managed: true,
        })?;
        set_env("SHIPSHAPE_KEEP_WORKSPACE", "false");
        let _keep_false_client = GitCommandClient::from_env();
        remove_env("SHIPSHAPE_KEEP_WORKSPACE");
        let client = GitCommandClient::from_env();
        let missing_dir = temp_dir();
        let repo_no_base = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: String::new(),
            local_path: Some(missing_dir.to_string_lossy().to_string()),
        };
        assert!(client.prepare_workspace(&repo_no_base).is_err());

        let repo_missing_local = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: "main".to_string(),
            local_path: Some(missing_dir.to_string_lossy().to_string()),
        };
        assert!(client.prepare_workspace(&repo_missing_local).is_err());

        let repo_empty_url = RepoSpec {
            repo_url: String::new(),
            base_branch: "main".to_string(),
            local_path: None,
        };
        assert!(client.prepare_workspace(&repo_empty_url).is_err());

        let root = temp_dir();
        std::fs::create_dir_all(&root)?;
        let repo_not_git = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: "main".to_string(),
            local_path: Some(root.to_string_lossy().to_string()),
        };
        assert!(client.prepare_workspace(&repo_not_git).is_err());
        std::fs::remove_dir_all(&root)?;

        let git_root = temp_dir();
        std::fs::create_dir_all(&git_root)?;
        run_git(&git_root, &["init", "-q"], &[])?;
        let repo_local_git = RepoSpec {
            repo_url: "https://github.com/shipshape/demo.git".to_string(),
            base_branch: "main".to_string(),
            local_path: Some(git_root.to_string_lossy().to_string()),
        };
        let ws_local = client.prepare_workspace(&repo_local_git)?;
        assert!(!ws_local.managed);
        client.cleanup(ws_local)?;
        std::fs::remove_dir_all(&git_root)?;

        // Full git flow (even when GIT_INDEX_FILE is set by an enclosing git hook)
        let prev_git_index = std::env::var("GIT_INDEX_FILE").ok();
        let workspace_root = temp_dir();
        std::fs::create_dir_all(&workspace_root)?;
        let ambient_index = workspace_root.join("ambient.index");
        let ambient_index_str = ambient_index.to_string_lossy().to_string();
        set_env("GIT_INDEX_FILE", &ambient_index_str);

        let origin = init_bare_repo()?;
        let mirror = init_bare_repo()?;
        let work = init_work_repo(&origin)?;
        let ws_root_str = workspace_root.to_string_lossy().to_string();
        set_env("SHIPSHAPE_WORKSPACE_ROOT", &ws_root_str);

        let client = GitCommandClient::from_env();
        let repo = RepoSpec {
            repo_url: origin.to_string_lossy().to_string(),
            base_branch: "main".to_string(),
            local_path: None,
        };
        let workspace = client.prepare_workspace(&repo)?;

        let patch = PatchSpec {
            diff: make_patch(&work)?,
            branch: "shipshape/refit".to_string(),
            commit_message: "Apply patch".to_string(),
        };
        let commit = client.apply_patch(&workspace, &patch)?;
        assert!(!commit.is_empty());

        client.create_branch(&workspace, &repo.base_branch, &patch.branch)?;
        let remote_ref = client.push_branch(&workspace, &repo, &patch.branch)?;
        assert!(remote_ref.contains(&patch.branch));

        let gitlab = GitLabSpec {
            mirror_url: mirror.to_string_lossy().to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };
        let mirror_ref = client.push_mirror(&workspace, &gitlab, &patch.branch)?;
        assert!(mirror_ref.contains(&patch.branch));
        // Second push_mirror exercises set-url succeeding on existing remote
        let mirror_ref_2 = client.push_mirror(&workspace, &gitlab, &patch.branch)?;
        assert!(mirror_ref_2.contains(&patch.branch));

        client.cleanup(workspace)?;
        assert!(!ambient_index.exists());

        std::fs::remove_dir_all(&origin)?;
        std::fs::remove_dir_all(&mirror)?;
        std::fs::remove_dir_all(&work)?;
        std::fs::remove_dir_all(&workspace_root)?;
        remove_env("SHIPSHAPE_WORKSPACE_ROOT");
        crate::db::restore_env_var("GIT_INDEX_FILE", prev_git_index);
        Ok(())
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
            mirror_url: String::new(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };
        assert!(client.push_mirror(&workspace, &gitlab, "branch").is_err());
    }

    #[test]
    fn github_and_gitlab_api_clients_handle_all_branches() -> ServerResult<()> {
        let _guard = env_lock()?;
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

        // Missing GITHUB_TOKEN
        remove_env("GITHUB_TOKEN");
        let no_tok_gh = GitHubApiClient::from_env();
        assert!(no_tok_gh.open_pr(&repo, &pr, "branch").is_err());

        // GitHub success, HTTP error, and missing html_url
        let server = MockServer::start();
        set_env("GITHUB_TOKEN", "token");
        set_env("GITHUB_API_URL", server.url("").as_str());
        set_env("GITHUB_USER_AGENT", "shipshape-test");
        let gh_client = GitHubApiClient::from_env();

        let mut gh_mock = server.mock(|when, then| {
            when.method(POST).path("/repos/shipshape/demo/pulls");
            then.status(201)
                .header("content-type", "application/json")
                .json_body(
                    serde_json::json!({"html_url": "https://github.com/shipshape/demo/pull/1"}),
                );
        });
        let pr_url = gh_client.open_pr(&repo, &pr, "branch")?;
        assert!(pr_url.contains("pull/1"));
        gh_mock.assert();
        gh_mock.delete();

        let mut gh_err = server.mock(|when, then| {
            when.method(POST).path("/repos/shipshape/demo/pulls");
            then.status(400).body("bad");
        });
        assert!(gh_client.open_pr(&repo, &pr, "branch").is_err());
        gh_err.assert();
        gh_err.delete();

        let mut gh_missing = server.mock(|when, then| {
            when.method(POST).path("/repos/shipshape/demo/pulls");
            then.status(201)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"id": 1}));
        });
        assert!(gh_client.open_pr(&repo, &pr, "branch").is_err());
        gh_missing.assert();
        gh_missing.delete();

        // GitLab API client tests
        let gitlab = GitLabSpec {
            mirror_url: "https://gitlab.example.com/shipshape/demo.git".to_string(),
            project_path: "shipshape/demo".to_string(),
            pipeline_ref: None,
        };
        remove_env("GITLAB_TOKEN");
        let no_tok_gl = GitLabApiClient::from_env();
        assert!(no_tok_gl.ensure_project(&gitlab).is_err());
        assert!(no_tok_gl.trigger_pipeline(&gitlab, "branch").is_err());

        set_env("GITLAB_TOKEN", "token");
        set_env("GITLAB_API_URL", server.url("").as_str());
        let gl_client = GitLabApiClient::from_env();

        let empty_gl = GitLabSpec {
            mirror_url: "https://gitlab.example.com/shipshape/demo.git".to_string(),
            project_path: String::new(),
            pipeline_ref: None,
        };
        assert!(gl_client.ensure_project(&empty_gl).is_err());
        assert!(gl_client.trigger_pipeline(&empty_gl, "branch").is_err());

        // trigger_pipeline: web_url, id fallback, error, and missing web_url
        let mut gl_pipe_ok = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v4/projects/shipshape%2Fdemo/pipeline");
            then.status(201)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "web_url": "https://gitlab.example.com/shipshape/demo/-/pipelines/1"
                }));
        });
        assert!(
            gl_client
                .trigger_pipeline(&gitlab, "branch")?
                .contains("pipelines/1")
        );
        gl_pipe_ok.assert();
        gl_pipe_ok.delete();

        let mut gl_pipe_id = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v4/projects/shipshape%2Fdemo/pipeline");
            then.status(201)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"id": 22}));
        });
        assert!(
            gl_client
                .trigger_pipeline(&gitlab, "branch")?
                .contains("pipelines/22")
        );
        gl_pipe_id.assert();
        gl_pipe_id.delete();

        let mut gl_pipe_err = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v4/projects/shipshape%2Fdemo/pipeline");
            then.status(400).body("bad");
        });
        assert!(gl_client.trigger_pipeline(&gitlab, "branch").is_err());
        gl_pipe_err.assert();
        gl_pipe_err.delete();

        let mut gl_pipe_empty = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v4/projects/shipshape%2Fdemo/pipeline");
            then.status(201)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({}));
        });
        assert!(gl_client.trigger_pipeline(&gitlab, "branch").is_err());
        gl_pipe_empty.assert();
        gl_pipe_empty.delete();

        // ensure_project: exists (200), lookup 500, create (404 -> 201), namespace 500, namespace not found, create 500
        let mut gl_lookup_200 = server.mock(|when, then| {
            when.method(GET).path("/api/v4/projects/shipshape%2Fdemo");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"id": 1}));
        });
        gl_client.ensure_project(&gitlab)?;
        gl_lookup_200.assert();
        gl_lookup_200.delete();

        let mut gl_lookup_500 = server.mock(|when, then| {
            when.method(GET).path("/api/v4/projects/shipshape%2Fdemo");
            then.status(500).body("boom");
        });
        assert!(gl_client.ensure_project(&gitlab).is_err());
        gl_lookup_500.assert();
        gl_lookup_500.delete();

        let gl_lookup_404 = server.mock(|when, then| {
            when.method(GET).path("/api/v4/projects/shipshape%2Fdemo");
            then.status(404);
        });
        let mut gl_ns_500 = server.mock(|when, then| {
            when.method(GET).path("/api/v4/namespaces");
            then.status(500).body("ns boom");
        });
        assert!(gl_client.ensure_project(&gitlab).is_err());
        gl_ns_500.delete();

        let mut gl_ns_empty = server.mock(|when, then| {
            when.method(GET).path("/api/v4/namespaces");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!([]));
        });
        assert!(gl_client.ensure_project(&gitlab).is_err());
        gl_ns_empty.delete();

        let _gl_ns_ok = server.mock(|when, then| {
            when.method(GET).path("/api/v4/namespaces");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!([{"id": 42, "full_path": "shipshape"}]));
        });
        let mut gl_create_500 = server.mock(|when, then| {
            when.method(POST).path("/api/v4/projects");
            then.status(500).body("create boom");
        });
        assert!(gl_client.ensure_project(&gitlab).is_err());
        gl_create_500.delete();

        let mut gl_create_201 = server.mock(|when, then| {
            when.method(POST).path("/api/v4/projects");
            then.status(201)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"id": 99}));
        });
        gl_client.ensure_project(&gitlab)?;
        gl_lookup_404.assert_calls(4);
        gl_create_201.assert();
        gl_create_201.delete();

        let mut gl_create_409 = server.mock(|when, then| {
            when.method(POST).path("/api/v4/projects");
            then.status(409)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"message": "already exists"}));
        });
        gl_client.ensure_project(&gitlab)?;
        gl_create_409.assert();
        gl_create_409.delete();

        let _gl_create_root_201 = server.mock(|when, then| {
            when.method(POST).path("/api/v4/projects");
            then.status(201)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"id": 100}));
        });

        // Root-level project without namespace
        let root_gitlab = GitLabSpec {
            mirror_url: "https://gitlab.example.com/demo.git".to_string(),
            project_path: "demo".to_string(),
            pipeline_ref: None,
        };
        let _gl_root_404 = server.mock(|when, then| {
            when.method(GET).path("/api/v4/projects/demo");
            then.status(404);
        });
        gl_client.ensure_project(&root_gitlab)?;

        remove_env("GITHUB_TOKEN");
        remove_env("GITHUB_API_URL");
        remove_env("GITHUB_USER_AGENT");
        remove_env("GITLAB_TOKEN");
        remove_env("GITLAB_API_URL");
        Ok(())
    }

    #[test]
    fn workflow_runner_fails_at_each_step() {
        let request = sample_request();

        for (fail_step, skip_idx) in [
            (WorkflowStepKind::ApplyPatch, 1),
            (WorkflowStepKind::CreateBranch, 2),
            (WorkflowStepKind::PushBranch, 3),
            (WorkflowStepKind::MirrorPush, 5),
        ] {
            let runner = WorkflowRunner::new(WorkflowClients {
                git: Arc::new(FailingGitClient::new(fail_step)),
                github: Arc::new(MockGitHubClient),
                gitlab: Arc::new(MockGitLabClient),
            });
            let result = runner.run(&request);
            assert_eq!(result.status, WorkflowStatus::Failed);
            assert!(
                result
                    .steps
                    .iter()
                    .skip(skip_idx)
                    .all(|step| step.status == WorkflowStatus::Skipped)
            );
        }

        // Exercise FailingGitClient succeeding through all git steps up to OpenPr
        let runner = WorkflowRunner::new(WorkflowClients {
            git: Arc::new(FailingGitClient::new(WorkflowStepKind::OpenPr)),
            github: Arc::new(FailingGitHubClient),
            gitlab: Arc::new(MockGitLabClient),
        });
        let result = runner.run(&request);
        assert_eq!(result.status, WorkflowStatus::Failed);
        assert_eq!(result.steps[6].status, WorkflowStatus::Failed);

        let runner = WorkflowRunner::new(WorkflowClients {
            git: Arc::new(MockGitClient),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(FailingGitLabClient { fail_ensure: true }),
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
            git: Arc::new(MockGitClient),
            github: Arc::new(MockGitHubClient),
            gitlab: Arc::new(FailingGitLabClient { fail_ensure: false }),
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
        assert_eq!(result.steps[6].status, WorkflowStatus::Failed);
    }

    #[test]
    fn run_git_reports_errors_with_redacted_credentials() {
        let err_msg = run_git(
            Path::new("/tmp"),
            &[
                "clone",
                "https://x-access-token:supersecret@github.com/missing/repo.git",
                "/dev/null/impossible",
            ],
            &[],
        )
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
        assert!(!err_msg.contains("supersecret"));
        assert!(err_msg.contains("https://***@github.com"));

        // Exercise stdout fallback when stderr is empty (`git diff --quiet` returns exit status 1 with no stderr)
        let quiet_args = ["diff", "--quiet", "/etc/hosts", "/etc/passwd"];
        let _ = run_git(Path::new("/tmp"), &quiet_args, &[]);
    }

    #[test]
    fn workflow_service_respects_mode() -> ServerResult<()> {
        let _guard = env_lock()?;
        remove_env("SHIPSHAPE_WORKFLOW_MODE");
        let _live = WorkflowService::from_env();

        set_env("SHIPSHAPE_WORKFLOW_MODE", "mock");
        let service = WorkflowService::from_env();
        let (pool, _db) = test_pool()?;
        let result = service.run(&pool, "vessel-1", &sample_request())?;
        assert_eq!(result.status, WorkflowStatus::Success);
        remove_env("SHIPSHAPE_WORKFLOW_MODE");
        Ok(())
    }

    /// Generate a unique temporary directory path.
    fn temp_dir() -> PathBuf {
        let pid = std::process::id();
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("shipshape_server_test_{pid}_{count}"))
    }

    /// Initialize a bare Git repository for push/clone tests.
    fn init_bare_repo() -> ServerResult<PathBuf> {
        let root = temp_dir();
        std::fs::create_dir_all(&root)?;
        run_git(&root, &["init", "--bare", "-q"], &[])?;
        Ok(root)
    }

    /// Initialize a working Git repository connected to `origin`.
    fn init_work_repo(origin: &Path) -> ServerResult<PathBuf> {
        let root = temp_dir();
        std::fs::create_dir_all(&root)?;
        run_git(&root, &["init", "-q"], &[])?;
        run_git(&root, &["checkout", "-q", "-b", "main"], &[])?;
        let readme = root.join("README.md");
        std::fs::write(&readme, "shipshape")?;
        run_git(&root, &["add", "."], &[])?;
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
        run_git(&root, &commit_args, &[])?;
        let origin_str = origin.to_string_lossy();
        let remote_args = ["remote", "add", "origin", origin_str.as_ref()];
        run_git(&root, &remote_args, &[])?;
        run_git(&root, &["push", "-q", "-u", "origin", "main"], &[])?;
        Ok(root)
    }

    /// Modify `README.md` in `repo` and return the `git diff` output.
    fn make_patch(repo: &Path) -> ServerResult<String> {
        let readme = repo.join("README.md");
        std::fs::write(&readme, "shipshape\nrefit\n")?;
        run_git(repo, &["diff"], &[])
    }

    /// Acquire the environment-variable test lock.
    fn env_lock() -> ServerResult<std::sync::MutexGuard<'static, ()>> {
        crate::db::db_env_lock()
    }

    /// Set environment variable `key` to `value`.
    fn set_env(key: &str, value: &str) {
        unsafe {
            std::env::set_var(key, value);
        }
    }

    /// Remove environment variable `key`.
    fn remove_env(key: &str) {
        unsafe {
            std::env::remove_var(key);
        }
    }

    /// Create an isolated test database and connection pool.
    fn test_pool() -> ServerResult<(DbPool, crate::db::TestDatabase)> {
        let mut test_db = crate::db::TestDatabase::new()?;
        let pool = test_db.pool()?;
        Ok((pool, test_db))
    }

    /// GitClient stub configured to fail at a specific [`WorkflowStepKind`].
    #[derive(Clone)]
    struct FailingGitClient {
        /// Step at which to return an error.
        fail_at: WorkflowStepKind,
    }

    impl FailingGitClient {
        /// Create a new failing git client for `fail_at`.
        const fn new(fail_at: WorkflowStepKind) -> Self {
            Self { fail_at }
        }
    }

    impl GitClient for FailingGitClient {
        fn prepare_workspace(&self, _repo: &RepoSpec) -> ServerResult<GitWorkspace> {
            Ok(GitWorkspace {
                path: PathBuf::from("/tmp/shipshape-mock"),
                managed: false,
            })
        }

        fn apply_patch(
            &self,
            _workspace: &GitWorkspace,
            _patch: &PatchSpec,
        ) -> ServerResult<String> {
            if self.fail_at == WorkflowStepKind::ApplyPatch {
                return Err(ServerError::workflow("fail apply"));
            }
            Ok("commit".to_string())
        }

        fn create_branch(
            &self,
            _workspace: &GitWorkspace,
            _base_branch: &str,
            _branch: &str,
        ) -> ServerResult<()> {
            if self.fail_at == WorkflowStepKind::CreateBranch {
                return Err(ServerError::workflow("fail create"));
            }
            Ok(())
        }

        fn push_branch(
            &self,
            _workspace: &GitWorkspace,
            _repo: &RepoSpec,
            _branch: &str,
        ) -> ServerResult<String> {
            if self.fail_at == WorkflowStepKind::PushBranch {
                return Err(ServerError::workflow("fail push"));
            }
            Ok("origin/branch".to_string())
        }

        fn push_mirror(
            &self,
            _workspace: &GitWorkspace,
            _gitlab: &GitLabSpec,
            _branch: &str,
        ) -> ServerResult<String> {
            if self.fail_at == WorkflowStepKind::MirrorPush {
                Err(ServerError::workflow("fail mirror"))
            } else {
                Ok("mirror/branch".to_string())
            }
        }

        fn cleanup(&self, _workspace: GitWorkspace) -> ServerResult<()> {
            Ok(())
        }
    }

    /// GitHubClient stub that always fails `open_pr`.
    #[derive(Clone)]
    struct FailingGitHubClient;

    impl GitHubClient for FailingGitHubClient {
        fn open_pr(
            &self,
            _repo: &RepoSpec,
            _pr: &PullRequestSpec,
            _branch: &str,
        ) -> ServerResult<String> {
            Err(ServerError::workflow("fail pr"))
        }
    }

    /// GitLabClient stub that fails either `ensure_project` or `trigger_pipeline`.
    #[derive(Clone)]
    struct FailingGitLabClient {
        /// Whether to fail `ensure_project` (`true`) or `trigger_pipeline` (`false`).
        fail_ensure: bool,
    }

    impl GitLabClient for FailingGitLabClient {
        fn ensure_project(&self, _gitlab: &GitLabSpec) -> ServerResult<()> {
            if self.fail_ensure {
                return Err(ServerError::workflow("fail ensure"));
            }
            Ok(())
        }

        fn trigger_pipeline(&self, _gitlab: &GitLabSpec, _branch: &str) -> ServerResult<String> {
            Err(ServerError::workflow("fail pipeline"))
        }
    }
}
