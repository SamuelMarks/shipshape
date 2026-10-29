//! HTTP handlers for ShipShape server.
//!
//! Implements OAuth authentication (`/api/auth/*`), fleet dashboard and batch
//! runs (`/api/dashboard`, `/api/batch/runs`), interactive diff editing
//! (`/api/diffs`), control-room orchestration (`/api/control/*`), and
//! database-backed voyage and vessel lifecycle endpoints (`/api/voyage/*`,
//! `/api/vessel/*`).

use std::sync::{Arc, RwLock};

use actix_web::{HttpRequest, HttpResponse, get, post, web};
use chrono::Utc;
use diesel::prelude::*;
use diesel::result::OptionalExtension;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::crypto::TokenCipher;
use crate::db::DbPool;
use crate::error::{ServerError, ServerResult};
use crate::models::{
    AuthSession, Diagnostic, Launch, NewAuthSession, NewUser, Refit, User, Vessel, Voyage,
};
use crate::openapi::ApiDoc;
use crate::schema::{auth_sessions, diagnostics, launches, refits, users, vessels, voyages};
use crate::workflows::{WorkflowRequest, WorkflowResult, WorkflowService};

/// Shared application state for Actix HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub pool: DbPool,
    /// Workflow orchestration service.
    pub workflow: WorkflowService,
    /// Authentication configuration.
    pub auth: AuthConfig,
    /// Token encryption helper for storing OAuth secrets.
    pub token_cipher: TokenCipher,
    /// Shared HTTP client for GitHub OAuth exchanges.
    pub http_client: Client,
    /// In-memory diff listing and edits.
    pub diff_store: Arc<RwLock<Vec<DiffFile>>>,
}

/// Authentication configuration loaded from the environment.
#[derive(Clone)]
pub struct AuthConfig {
    /// GitHub OAuth client ID.
    pub github_client_id: String,
    /// GitHub OAuth client secret.
    pub github_client_secret: String,
    /// GitHub OAuth base URL.
    pub github_oauth_url: String,
    /// GitHub API base URL.
    pub github_api_url: String,
    /// GitHub API `User-Agent` string.
    pub github_user_agent: String,
    /// UI base URL for OAuth redirect.
    pub ui_base_url: String,
    /// GitHub OAuth scopes.
    pub github_scopes: Vec<String>,
}

impl AuthConfig {
    /// Build auth config from environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        let scopes_raw =
            std::env::var("GITHUB_SCOPES").unwrap_or_else(|_| "read:user,repo".to_string());
        let github_scopes = scopes_raw
            .split(',')
            .map(|scope| scope.trim().to_string())
            .filter(|scope| !scope.is_empty())
            .collect();
        Self {
            github_client_id: std::env::var("GITHUB_CLIENT_ID").unwrap_or_default(),
            github_client_secret: std::env::var("GITHUB_CLIENT_SECRET").unwrap_or_default(),
            github_oauth_url: std::env::var("GITHUB_OAUTH_URL")
                .unwrap_or_else(|_| "https://github.com/login/oauth".to_string()),
            github_api_url: std::env::var("GITHUB_API_URL")
                .unwrap_or_else(|_| "https://api.github.com".to_string()),
            github_user_agent: std::env::var("GITHUB_USER_AGENT")
                .unwrap_or_else(|_| "shipshape-server".to_string()),
            ui_base_url: std::env::var("SHIPSHAPE_UI_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:4200".to_string()),
            github_scopes,
        }
    }

    /// Return the GitHub OAuth authorization endpoint URL.
    fn authorize_url(&self) -> String {
        format!("{}/authorize", self.github_oauth_url.trim_end_matches('/'))
    }

    /// Return the GitHub OAuth token exchange endpoint URL.
    fn token_url(&self) -> String {
        format!(
            "{}/access_token",
            self.github_oauth_url.trim_end_matches('/')
        )
    }

    /// Return the default UI callback redirect URI.
    fn redirect_uri(&self) -> String {
        format!("{}/auth/callback", self.ui_base_url.trim_end_matches('/'))
    }
}

/// Verified user context extracted from an authenticated request.
#[derive(Clone)]
struct AuthContext {
    /// Authenticated user record.
    user: User,
}

/// OAuth configuration shared with the UI.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigResponse {
    /// GitHub OAuth client ID.
    pub client_id: String,
    /// GitHub authorize URL.
    pub authorize_url: String,
    /// OAuth scopes requested.
    pub scopes: Vec<String>,
    /// Redirect URI for the UI callback.
    pub redirect_uri: String,
}

/// Authenticated user profile.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthUser {
    /// ShipShape user identifier.
    pub id: String,
    /// GitHub login handle.
    pub login: String,
    /// GitHub user ID.
    pub github_id: String,
}

/// Request payload for GitHub OAuth exchange.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthGithubRequest {
    /// GitHub OAuth code.
    pub code: String,
    /// Optional redirect URI override.
    pub redirect_uri: Option<String>,
}

/// Request payload for exchanging a GitHub token.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthGithubTokenRequest {
    /// GitHub OAuth access token.
    pub access_token: String,
}

/// Response payload for GitHub OAuth exchange.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthGithubResponse {
    /// ShipShape access token.
    pub token: String,
    /// Authenticated user profile.
    pub user: AuthUser,
}

/// Request payload for boarding a voyage.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VoyageBoardRequest {
    /// Repository URLs to include in the voyage.
    pub repos: Vec<String>,
}

/// Response payload for a boarded voyage.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VoyageBoardResponse {
    /// Generated voyage identifier.
    pub voyage_id: String,
    /// Current status of the voyage.
    pub status: String,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// Diagnostics summary payload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiagnosticsReport {
    /// Summary text for diagnostics.
    pub summary: String,
}

/// Response payload for vessel diagnostics.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiagnosticsResponse {
    /// Vessel identifier.
    pub vessel_id: String,
    /// Diagnostics status.
    pub status: String,
    /// Diagnostics report payload.
    pub report: DiagnosticsReport,
}

/// Request payload for refit actions.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RefitRequest {
    /// Whether to apply fixes instead of dry-run.
    pub apply: bool,
}

/// Response payload for refit status.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RefitResponse {
    /// Vessel identifier.
    pub vessel_id: String,
    /// Refit status message.
    pub status: String,
}

/// Error response payload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    /// Error message.
    pub message: String,
}

/// Request payload for launch actions.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LaunchRequest {
    /// Voyage identifier to launch.
    pub voyage_id: String,
}

/// Response payload for launch status.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LaunchResponse {
    /// Voyage identifier.
    pub voyage_id: String,
    /// Launch status message.
    pub status: String,
}

/// Fleet metric for the dashboard.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FleetMetric {
    /// Metric label.
    pub label: String,
    /// Metric value.
    pub value: String,
    /// Optional trend text.
    pub trend: Option<String>,
    /// Optional detail text.
    pub detail: Option<String>,
    /// Metric tone.
    pub tone: String,
}

/// Vessel status for the dashboard.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VesselStatus {
    /// Vessel identifier.
    pub id: String,
    /// Vessel display name.
    pub name: String,
    /// Health score value.
    pub health_score: u8,
    /// Coverage risk label.
    pub coverage_risk: String,
    /// Last run description.
    pub last_run: String,
    /// Status tone.
    pub tone: String,
    /// Status label.
    pub status_label: String,
}

/// Alert for the dashboard.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FleetAlert {
    /// Alert identifier.
    pub id: String,
    /// Alert title.
    pub title: String,
    /// Alert description.
    pub description: String,
    /// Alert tone.
    pub tone: String,
}

/// Dashboard response payload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardResponse {
    /// Summary metrics.
    pub metrics: Vec<FleetMetric>,
    /// Vessel list.
    pub vessels: Vec<VesselStatus>,
    /// Alerts.
    pub alerts: Vec<FleetAlert>,
}

/// Batch run entry.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchRun {
    /// Batch identifier.
    pub id: String,
    /// Batch label.
    pub label: String,
    /// Owning team.
    pub owner: String,
    /// Target repo count.
    pub target_count: u32,
    /// Batch status.
    pub status: String,
    /// Health summary.
    pub health: String,
    /// Last run description.
    pub last_run: String,
    /// Status tone.
    pub tone: String,
}

/// Batch runs response payload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchRunsResponse {
    /// Batch run entries.
    pub runs: Vec<BatchRun>,
}

/// Diff file entry.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    /// File path.
    pub path: String,
    /// Summary description.
    pub summary: String,
    /// Language identifier.
    pub language: String,
    /// Original content.
    pub original: String,
    /// Modified content.
    pub modified: String,
    /// Status tone.
    pub tone: String,
    /// Status label.
    pub status_label: String,
}

/// Diff listing response payload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffListingResponse {
    /// Diff files.
    pub files: Vec<DiffFile>,
}

/// Request payload for updating a diff entry.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffUpdateRequest {
    /// File path to update.
    pub path: String,
    /// Updated modified content.
    pub modified: String,
}

/// Response payload for a diff update.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffUpdateResponse {
    /// Updated diff file entry.
    pub file: DiffFile,
}

/// Seed the in-memory diff store with default content.
#[must_use]
pub fn seed_diff_store() -> Arc<RwLock<Vec<DiffFile>>> {
    Arc::new(RwLock::new(default_diff_files()))
}

/// Build the default diff files for the interactive diff viewer.
fn default_diff_files() -> Vec<DiffFile> {
    vec![
        DiffFile {
            path: "src/inspector.rs".to_string(),
            summary: "Added health score heuristics for coverage risk.".to_string(),
            language: "rust".to_string(),
            original: "pub fn health_score(report: &FleetReport) -> u8 {\n  0\n}\n".to_string(),
            modified: "pub fn health_score(report: &FleetReport) -> u8 {\n  let mut score = 70;\n  if report.coverage.low_count > 0 {\n    score = score.saturating_sub(20);\n  }\n  score\n}\n".to_string(),
            tone: "good".to_string(),
            status_label: "Modified".to_string(),
        },
        DiffFile {
            path: "src/drydock.rs".to_string(),
            summary: "Added CMake detection and notebook-only guardrails.".to_string(),
            language: "rust".to_string(),
            original: "pub fn detect_stack(files: &[String]) -> Stack {\n  Stack::Unknown\n}\n".to_string(),
            modified: r#"pub fn detect_stack(files: &[String]) -> Stack {
  if files.iter().any(|name| name.contains("CMakeLists")) {
    return Stack::Cmake;
  }
  Stack::Unknown
}
"#.to_string(),
            tone: "warn".to_string(),
            status_label: "Modified".to_string(),
        },
        DiffFile {
            path: "src/pr_template.rs".to_string(),
            summary: "Interpolated ShipShape stats into PR templates.".to_string(),
            language: "rust".to_string(),
            original: r#"const SHIPSHAPE_STATS: &str = "{{SHIPSHAPE_STATS}}";
"#.to_string(),
            modified: r#"const SHIPSHAPE_STATS: &str = "{{SHIPSHAPE_STATS}}";
const SHIPSHAPE_FIXES: &str = "{{SHIPSHAPE_FIXES}}";
"#.to_string(),
            tone: "info".to_string(),
            status_label: "Added".to_string(),
        },
    ]
}

/// Mechanic option entry.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MechanicOption {
    /// Mechanic identifier.
    pub id: String,
    /// Mechanic label.
    pub label: String,
    /// Mechanic description.
    pub description: String,
}

/// Activity log entry.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLog {
    /// Log identifier.
    pub id: String,
    /// Log title.
    pub title: String,
    /// Log detail.
    pub detail: String,
    /// Relative time description.
    pub time: String,
    /// Status tone.
    pub tone: String,
}

/// Control room options payload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ControlOptionsResponse {
    /// Available mechanics.
    pub mechanics: Vec<MechanicOption>,
    /// Recent activity.
    pub activity: Vec<ActivityLog>,
}

/// Request payload for queuing a control run.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ControlQueueRequest {
    /// Source type.
    pub source_type: String,
    /// Source value.
    pub source_value: String,
    /// Run mode.
    pub mode: String,
    /// Whether this is a dry run.
    pub dry_run: bool,
    /// Selected mechanic IDs.
    pub mechanic_ids: Vec<String>,
}

/// Response payload for queued control runs.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ControlQueueResponse {
    /// Run identifier.
    pub run_id: String,
    /// Activity log entry.
    pub log: ActivityLog,
}

/// Request payload sent to GitHub's `/login/oauth/access_token` endpoint.
#[derive(Serialize)]
struct GithubTokenRequest {
    /// OAuth client ID.
    client_id: String,
    /// OAuth client secret.
    client_secret: String,
    /// Authorization code from callback.
    code: String,
    /// Optional redirect URI.
    redirect_uri: Option<String>,
}

/// Response payload received from GitHub's `/login/oauth/access_token` endpoint.
#[derive(Deserialize)]
struct GithubTokenResponse {
    /// Issued GitHub OAuth access token.
    access_token: String,
}

/// Response payload received from GitHub's `/user` endpoint.
#[derive(Deserialize)]
struct GithubUserResponse {
    /// Numeric GitHub user ID.
    id: u64,
    /// GitHub login username.
    login: String,
}

/// Extract a non-empty `Bearer <token>` from `req`'s `Authorization` header.
///
/// # Arguments
/// * `req` - Incoming HTTP request.
fn extract_bearer_token(req: &HttpRequest) -> ServerResult<String> {
    let header = req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or(ServerError::Unauthorized {
            reason: "missing authorization header",
        })?;
    let token = header
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ServerError::Unauthorized {
            reason: "missing bearer token",
        })?;
    Ok(token.to_string())
}

/// Verify the request's bearer token against PostgreSQL and update `last_used_at`.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
async fn require_auth(state: &web::Data<AppState>, req: &HttpRequest) -> ServerResult<AuthContext> {
    let token = extract_bearer_token(req)?;
    let pool = state.pool.clone();
    web::block(move || {
        let mut conn = pool.get()?;
        let session = auth_sessions::table
            .filter(auth_sessions::shipshape_token.eq(&token))
            .first::<AuthSession>(&mut conn)
            .optional()?
            .ok_or(ServerError::Unauthorized {
                reason: "invalid session",
            })?;
        let now = Utc::now().naive_utc();
        diesel::update(auth_sessions::table.filter(auth_sessions::id.eq(&session.id)))
            .set(auth_sessions::last_used_at.eq(now))
            .execute(&mut conn)?;
        let user = users::table
            .filter(users::id.eq(&session.user_id))
            .first::<User>(&mut conn)?;
        Ok::<AuthContext, ServerError>(AuthContext { user })
    })
    .await?
}

/// Exchange an OAuth authorization `code` for a GitHub access token.
///
/// # Arguments
/// * `client` - Shared async HTTP client.
/// * `auth` - Server OAuth configuration.
/// * `payload` - Incoming code exchange request.
async fn exchange_github_code(
    client: &Client,
    auth: &AuthConfig,
    payload: &AuthGithubRequest,
) -> ServerResult<String> {
    let redirect_uri = payload
        .redirect_uri
        .clone()
        .or_else(|| Some(auth.redirect_uri()));
    let request = GithubTokenRequest {
        client_id: auth.github_client_id.clone(),
        client_secret: auth.github_client_secret.clone(),
        code: payload.code.clone(),
        redirect_uri,
    };
    let response = client
        .post(auth.token_url())
        .header("Accept", "application/json")
        .json(&request)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let detail = "GitHub token exchange failed".to_string();
        return Err(ServerError::GitHubApi {
            status: status.to_string(),
            detail,
        });
    }
    let body: GithubTokenResponse = response.json().await?;
    Ok(body.access_token)
}

/// Fetch the authenticated GitHub user profile using `token`.
///
/// # Arguments
/// * `client` - Shared async HTTP client.
/// * `auth` - Server OAuth configuration.
/// * `token` - GitHub OAuth access token.
async fn fetch_github_user(
    client: &Client,
    auth: &AuthConfig,
    token: &str,
) -> ServerResult<GithubUserResponse> {
    let url = format!("{}/user", auth.github_api_url.trim_end_matches('/'));
    let response = client
        .get(url)
        .header("User-Agent", auth.github_user_agent.clone())
        .bearer_auth(token)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let detail = "GitHub user lookup failed".to_string();
        return Err(ServerError::GitHubApi {
            status: status.to_string(),
            detail,
        });
    }
    Ok(response.json().await?)
}

/// Persist a ShipShape auth session from a GitHub profile and token.
///
/// # Arguments
/// * `conn` - Active PostgreSQL connection.
/// * `token_cipher` - AES-256-GCM cipher for encrypting `github_token`.
/// * `github_user` - GitHub user profile.
/// * `github_token` - Raw GitHub access token.
fn persist_auth_session(
    conn: &mut diesel::pg::PgConnection,
    token_cipher: &TokenCipher,
    github_user: GithubUserResponse,
    github_token: &str,
) -> ServerResult<AuthGithubResponse> {
    let now = Utc::now().naive_utc();
    let existing = users::table
        .filter(users::github_id.eq(github_user.id.to_string()))
        .first::<User>(conn)
        .optional()?;
    let user = if let Some(user) = existing {
        user
    } else {
        let new_user = NewUser {
            id: Uuid::new_v4().to_string(),
            github_id: github_user.id.to_string(),
            github_login: github_user.login,
            created_at: now,
        };
        diesel::insert_into(users::table)
            .values(&new_user)
            .execute(conn)?;
        User {
            id: new_user.id,
            github_id: new_user.github_id,
            github_login: new_user.github_login,
            created_at: new_user.created_at,
        }
    };
    let shipshape_token = Uuid::new_v4().to_string();
    let encrypted_github_token = token_cipher.encrypt(github_token)?;
    let session = NewAuthSession {
        id: Uuid::new_v4().to_string(),
        user_id: user.id.clone(),
        shipshape_token: shipshape_token.clone(),
        github_token: encrypted_github_token,
        created_at: now,
        last_used_at: now,
    };
    diesel::insert_into(auth_sessions::table)
        .values(&session)
        .execute(conn)?;
    Ok(AuthGithubResponse {
        token: shipshape_token,
        user: AuthUser {
            id: user.id,
            login: user.github_login,
            github_id: user.github_id,
        },
    })
}

/// Configure all ShipShape API routes on `cfg`.
///
/// # Arguments
/// * `cfg` - Actix service configuration builder.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(auth_config)
        .service(auth_github)
        .service(auth_github_token)
        .service(auth_me)
        .service(dashboard)
        .service(batch_runs)
        .service(diffs)
        .service(diff_update)
        .service(control_options)
        .service(control_queue)
        .service(voyage_board)
        .service(vessel_diagnostics)
        .service(vessel_refit)
        .service(vessel_workflow)
        .service(voyage_launch)
        .service(openapi_json);
}

#[utoipa::path(
    get,
    path = "/auth/config",
    responses(
        (status = 200, description = "OAuth config", body = AuthConfigResponse)
    ),
    tag = "auth"
)]
#[get("/api/auth/config")]
/// Fetch OAuth configuration for the UI.
///
/// # Arguments
/// * `state` - Shared application state.
pub async fn auth_config(state: web::Data<AppState>) -> ServerResult<HttpResponse> {
    let config = &state.auth;
    Ok(HttpResponse::Ok().json(AuthConfigResponse {
        client_id: config.github_client_id.clone(),
        authorize_url: config.authorize_url(),
        scopes: config.github_scopes.clone(),
        redirect_uri: config.redirect_uri(),
    }))
}

#[utoipa::path(
    post,
    path = "/auth/github",
    request_body = AuthGithubRequest,
    responses(
        (status = 200, description = "Authenticated", body = AuthGithubResponse),
        (status = 500, description = "OAuth exchange failed", body = ErrorResponse)
    ),
    tag = "auth"
)]
#[post("/api/auth/github")]
/// Exchange a GitHub OAuth code for a ShipShape session.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `payload` - OAuth code exchange request.
pub async fn auth_github(
    state: web::Data<AppState>,
    payload: web::Json<AuthGithubRequest>,
) -> ServerResult<HttpResponse> {
    let payload = payload.into_inner();
    let github_token = exchange_github_code(&state.http_client, &state.auth, &payload).await?;
    let github_user = fetch_github_user(&state.http_client, &state.auth, &github_token).await?;
    let pool = state.pool.clone();
    let token_cipher = state.token_cipher.clone();
    let response = web::block(move || {
        let mut conn = pool.get()?;
        persist_auth_session(&mut conn, &token_cipher, github_user, &github_token)
    })
    .await??;

    Ok(HttpResponse::Ok().json(response))
}

#[utoipa::path(
    post,
    path = "/auth/github/token",
    request_body = AuthGithubTokenRequest,
    responses(
        (status = 200, description = "Authenticated", body = AuthGithubResponse),
        (status = 500, description = "Token exchange failed", body = ErrorResponse)
    ),
    tag = "auth"
)]
#[post("/api/auth/github/token")]
/// Exchange a GitHub access token for a ShipShape session.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `payload` - GitHub access token exchange request.
pub async fn auth_github_token(
    state: web::Data<AppState>,
    payload: web::Json<AuthGithubTokenRequest>,
) -> ServerResult<HttpResponse> {
    let payload = payload.into_inner();
    let github_user =
        fetch_github_user(&state.http_client, &state.auth, &payload.access_token).await?;
    let pool = state.pool.clone();
    let token_cipher = state.token_cipher.clone();
    let response = web::block(move || {
        let mut conn = pool.get()?;
        persist_auth_session(&mut conn, &token_cipher, github_user, &payload.access_token)
    })
    .await??;

    Ok(HttpResponse::Ok().json(response))
}

#[utoipa::path(
    get,
    path = "/auth/me",
    responses(
        (status = 200, description = "Authenticated user", body = AuthUser),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    tag = "auth"
)]
#[get("/api/auth/me")]
/// Fetch the current authenticated user.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
pub async fn auth_me(state: web::Data<AppState>, req: HttpRequest) -> ServerResult<HttpResponse> {
    let context = require_auth(&state, &req).await?;
    Ok(HttpResponse::Ok().json(AuthUser {
        id: context.user.id,
        login: context.user.github_login,
        github_id: context.user.github_id,
    }))
}

#[utoipa::path(
    get,
    path = "/dashboard",
    responses(
        (status = 200, description = "Dashboard data", body = DashboardResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    tag = "dashboard"
)]
#[get("/api/dashboard")]
/// Fetch dashboard data, combining persisted vessels with fleet baseline telemetry.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
pub async fn dashboard(state: web::Data<AppState>, req: HttpRequest) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let pool = state.pool.clone();
    let db_vessels: Vec<Vessel> = web::block(move || {
        let mut conn = pool.get()?;
        Ok::<Vec<Vessel>, ServerError>(vessels::table.load::<Vessel>(&mut conn)?)
    })
    .await??;

    let active_count = 128 + db_vessels.len();
    let metrics = vec![
        FleetMetric {
            label: "Active vessels".to_string(),
            value: active_count.to_string(),
            trend: Some("+6 this week".to_string()),
            detail: None,
            tone: "good".to_string(),
        },
        FleetMetric {
            label: "Open refit findings".to_string(),
            value: "42".to_string(),
            trend: None,
            detail: Some("Top risk: missing docs".to_string()),
            tone: "warn".to_string(),
        },
        FleetMetric {
            label: "Launch-ready".to_string(),
            value: "76%".to_string(),
            trend: Some("+4% from last cycle".to_string()),
            detail: None,
            tone: "good".to_string(),
        },
        FleetMetric {
            label: "Critical drifts".to_string(),
            value: "3".to_string(),
            trend: None,
            detail: Some("2 notebook-only stacks".to_string()),
            tone: "bad".to_string(),
        },
    ];
    let mut vessels = vec![
        VesselStatus {
            id: "VX-118".to_string(),
            name: "Apollo Navigation".to_string(),
            health_score: 92,
            coverage_risk: "Low".to_string(),
            last_run: "2 hours ago".to_string(),
            tone: "good".to_string(),
            status_label: "Healthy".to_string(),
        },
        VesselStatus {
            id: "VX-204".to_string(),
            name: "Kepler Analytics".to_string(),
            health_score: 78,
            coverage_risk: "Medium".to_string(),
            last_run: "4 hours ago".to_string(),
            tone: "warn".to_string(),
            status_label: "Watch".to_string(),
        },
        VesselStatus {
            id: "VX-330".to_string(),
            name: "Orion Payments".to_string(),
            health_score: 63,
            coverage_risk: "High".to_string(),
            last_run: "9 hours ago".to_string(),
            tone: "bad".to_string(),
            status_label: "Critical".to_string(),
        },
    ];
    for db_vessel in db_vessels {
        let name = db_vessel
            .repo_url
            .or(db_vessel.local_path)
            .unwrap_or(db_vessel.id.clone());
        vessels.push(VesselStatus {
            id: db_vessel.id,
            name,
            health_score: 90,
            coverage_risk: "Low".to_string(),
            last_run: "Just now".to_string(),
            tone: "good".to_string(),
            status_label: "Boarded".to_string(),
        });
    }
    let alerts = vec![
        FleetAlert {
            id: "AL-19".to_string(),
            title: "Drydock requires CMake adjustments".to_string(),
            description: "2 repos detected missing CMakeLists CI steps.".to_string(),
            tone: "warn".to_string(),
        },
        FleetAlert {
            id: "AL-24".to_string(),
            title: "Notebook-only refit blocked".to_string(),
            description: "1 repo needs notebook conversion before audit.".to_string(),
            tone: "bad".to_string(),
        },
        FleetAlert {
            id: "AL-31".to_string(),
            title: "Coverage threshold trending upward".to_string(),
            description: "Fleet health score improved 6 points this week.".to_string(),
            tone: "good".to_string(),
        },
    ];
    Ok(HttpResponse::Ok().json(DashboardResponse {
        metrics,
        vessels,
        alerts,
    }))
}

#[utoipa::path(
    get,
    path = "/batch/runs",
    responses(
        (status = 200, description = "Batch runs", body = BatchRunsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    tag = "batch"
)]
#[get("/api/batch/runs")]
/// Fetch batch runs, including persisted voyages from PostgreSQL.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
pub async fn batch_runs(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let pool = state.pool.clone();
    let db_voyages: Vec<Voyage> = web::block(move || {
        let mut conn = pool.get()?;
        Ok::<Vec<Voyage>, ServerError>(voyages::table.load::<Voyage>(&mut conn)?)
    })
    .await??;

    let mut runs = vec![
        BatchRun {
            id: "B-2201".to_string(),
            label: "Northstar compliance sweep".to_string(),
            owner: "Core Platform".to_string(),
            target_count: 42,
            status: "Running".to_string(),
            health: "89% healthy".to_string(),
            last_run: "12 minutes ago".to_string(),
            tone: "good".to_string(),
        },
        BatchRun {
            id: "B-2184".to_string(),
            label: "Notebook conversion wave".to_string(),
            owner: "Data Ops".to_string(),
            target_count: 18,
            status: "Queued".to_string(),
            health: "Pending staging".to_string(),
            last_run: "Scheduled".to_string(),
            tone: "info".to_string(),
        },
        BatchRun {
            id: "B-2147".to_string(),
            label: "C++ type safety refit".to_string(),
            owner: "Runtime".to_string(),
            target_count: 27,
            status: "Complete".to_string(),
            health: "93% healthy".to_string(),
            last_run: "3 hours ago".to_string(),
            tone: "good".to_string(),
        },
        BatchRun {
            id: "B-2109".to_string(),
            label: "Legacy CI drydock audit".to_string(),
            owner: "Core Platform".to_string(),
            target_count: 12,
            status: "Failed".to_string(),
            health: "4 repos blocked".to_string(),
            last_run: "Yesterday".to_string(),
            tone: "bad".to_string(),
        },
    ];
    for voyage in db_voyages {
        runs.push(BatchRun {
            id: voyage.id,
            label: "Boarded voyage".to_string(),
            owner: "Fleet Control".to_string(),
            target_count: 1,
            status: voyage.status,
            health: "Pending".to_string(),
            last_run: voyage.created_at.to_string(),
            tone: "info".to_string(),
        });
    }
    Ok(HttpResponse::Ok().json(BatchRunsResponse { runs }))
}

#[utoipa::path(
    get,
    path = "/diffs",
    responses(
        (status = 200, description = "Diff listing", body = DiffListingResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    tag = "diffs"
)]
#[get("/api/diffs")]
/// Fetch diff listing.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
pub async fn diffs(state: web::Data<AppState>, req: HttpRequest) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let guard = state
        .diff_store
        .read()
        .ok()
        .ok_or(ServerError::LockPoisoned {
            resource: "diff store",
        })?;
    let files = guard.clone();
    Ok(HttpResponse::Ok().json(DiffListingResponse { files }))
}

#[utoipa::path(
    post,
    path = "/diffs",
    request_body = DiffUpdateRequest,
    responses(
        (status = 200, description = "Diff update", body = DiffUpdateResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Diff not found", body = ErrorResponse)
    ),
    tag = "diffs"
)]
#[post("/api/diffs")]
/// Update a diff entry.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
/// * `payload` - Diff path and modified content.
pub async fn diff_update(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<DiffUpdateRequest>,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    if payload.path.trim().is_empty() {
        return Err(ServerError::BadRequest {
            message: "diff path is required".to_string(),
        });
    }
    let mut store = state
        .diff_store
        .write()
        .ok()
        .ok_or(ServerError::LockPoisoned {
            resource: "diff store",
        })?;
    if let Some(entry) = store.iter_mut().find(|file| file.path == payload.path) {
        entry.modified = payload.modified.clone();
        return Ok(HttpResponse::Ok().json(DiffUpdateResponse {
            file: entry.clone(),
        }));
    }
    Err(ServerError::NotFound {
        message: "diff file not found".to_string(),
    })
}

#[utoipa::path(
    get,
    path = "/control/options",
    responses(
        (status = 200, description = "Control room options", body = ControlOptionsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    tag = "control"
)]
#[get("/api/control/options")]
/// Fetch control room options.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
pub async fn control_options(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let mechanics = vec![
        MechanicOption {
            id: "cpp-types".to_string(),
            label: "C++ type safety".to_string(),
            description: "Enforce strict typing and migrate unsafe macros.".to_string(),
        },
        MechanicOption {
            id: "notebook-lib".to_string(),
            label: "Notebook library".to_string(),
            description: "Convert notebooks into installable libraries.".to_string(),
        },
        MechanicOption {
            id: "go-err".to_string(),
            label: "Go error handling".to_string(),
            description: "Thread error handling across call stacks.".to_string(),
        },
        MechanicOption {
            id: "ci-drydock".to_string(),
            label: "CI drydock".to_string(),
            description: "Generate Docker + GitLab CI verification.".to_string(),
        },
    ];
    let activity = vec![
        ActivityLog {
            id: "LG-1023".to_string(),
            title: "Launch wave queued".to_string(),
            detail: "Queued 12 repos for mirror validation.".to_string(),
            time: "11 minutes ago".to_string(),
            tone: "info".to_string(),
        },
        ActivityLog {
            id: "LG-1019".to_string(),
            title: "Refit completed".to_string(),
            detail: "Applied 4 mechanics across 8 repos.".to_string(),
            time: "1 hour ago".to_string(),
            tone: "good".to_string(),
        },
        ActivityLog {
            id: "LG-1014".to_string(),
            title: "Audit blocked".to_string(),
            detail: "Notebook-only repo needs conversion.".to_string(),
            time: "3 hours ago".to_string(),
            tone: "warn".to_string(),
        },
    ];
    Ok(HttpResponse::Ok().json(ControlOptionsResponse {
        mechanics,
        activity,
    }))
}

#[utoipa::path(
    post,
    path = "/control/queue",
    request_body = ControlQueueRequest,
    responses(
        (status = 200, description = "Control run queued", body = ControlQueueResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    tag = "control"
)]
#[post("/api/control/queue")]
/// Queue a control room run, validating selected mechanics and persisting a voyage record.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
/// * `payload` - Control queue request parameters.
pub async fn control_queue(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<ControlQueueRequest>,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let payload = payload.into_inner();
    let _validated_mechanics = shipshape_core::build_mechanics(&payload.mechanic_ids)?;
    let run_id = Uuid::new_v4().to_string();
    let pool = state.pool.clone();
    let mode_label = payload.mode.clone();
    let source_val = payload.source_value.clone();
    let run_id_db = run_id.clone();

    web::block(move || {
        let mut conn = pool.get()?;
        let now = Utc::now().naive_utc();
        let voyage = Voyage {
            id: run_id_db.clone(),
            status: format!("{mode_label} queued"),
            created_at: now,
        };
        diesel::insert_into(voyages::table)
            .values(&voyage)
            .execute(&mut conn)?;
        let vessel = Vessel {
            id: Uuid::new_v4().to_string(),
            voyage_id: run_id_db,
            repo_url: Some(source_val),
            local_path: None,
            created_at: now,
        };
        diesel::insert_into(vessels::table)
            .values(&vessel)
            .execute(&mut conn)?;
        Ok::<(), ServerError>(())
    })
    .await??;

    let detail = format!(
        "Source {} with {} mechanics selected.",
        payload.source_type,
        payload.mechanic_ids.len()
    );
    let log = ActivityLog {
        id: format!("LG-{}", 1200 + payload.mechanic_ids.len()),
        title: format!("{} queued", payload.mode),
        detail,
        time: "Just now".to_string(),
        tone: "info".to_string(),
    };
    Ok(HttpResponse::Ok().json(ControlQueueResponse { run_id, log }))
}

#[utoipa::path(
    post,
    path = "/voyage/board",
    request_body = VoyageBoardRequest,
    responses(
        (status = 200, description = "Voyage queued", body = VoyageBoardResponse)
    ),
    tag = "voyage"
)]
#[post("/api/voyage/board")]
/// Queue a voyage for the provided repositories and persist `Voyage` and `Vessel` rows.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
/// * `payload` - Repository URLs to board.
pub async fn voyage_board(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<VoyageBoardRequest>,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let repos = payload.into_inner().repos;
    let pool = state.pool.clone();
    let response = web::block(move || {
        let mut conn = pool.get()?;
        let now = Utc::now();
        let naive_now = now.naive_utc();
        let voyage_id = Uuid::new_v4().to_string();
        let status = format!("queued {} repos", repos.len());
        let new_voyage = Voyage {
            id: voyage_id.clone(),
            status: status.clone(),
            created_at: naive_now,
        };
        diesel::insert_into(voyages::table)
            .values(&new_voyage)
            .execute(&mut conn)?;

        let new_vessels: Vec<Vessel> = repos
            .into_iter()
            .map(|repo_url| Vessel {
                id: Uuid::new_v4().to_string(),
                voyage_id: voyage_id.clone(),
                repo_url: Some(repo_url),
                local_path: None,
                created_at: naive_now,
            })
            .collect();
        if !new_vessels.is_empty() {
            diesel::insert_into(vessels::table)
                .values(&new_vessels)
                .execute(&mut conn)?;
        }

        let persisted: Voyage = voyages::table.find(&voyage_id).first(&mut conn)?;
        Ok::<VoyageBoardResponse, ServerError>(VoyageBoardResponse {
            voyage_id: persisted.id,
            status: persisted.status,
            created_at: now.to_rfc3339(),
        })
    })
    .await??;

    Ok(HttpResponse::Ok().json(response))
}

#[utoipa::path(
    get,
    path = "/vessel/{id}/diagnostics",
    params(
        ("id" = String, Path, description = "Vessel identifier")
    ),
    responses(
        (status = 200, description = "Diagnostics report", body = DiagnosticsResponse)
    ),
    tag = "vessel"
)]
#[get("/api/vessel/{id}/diagnostics")]
/// Fetch or initialize diagnostics for a vessel in PostgreSQL.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
/// * `path` - Vessel identifier path parameter.
pub async fn vessel_diagnostics(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let vessel_id = path.into_inner();
    let pool = state.pool.clone();
    let response = web::block(move || {
        let mut conn = pool.get()?;
        let now = Utc::now().naive_utc();
        ensure_vessel_exists(&mut conn, &vessel_id, now)?;
        let existing = diagnostics::table
            .filter(diagnostics::vessel_id.eq(&vessel_id))
            .first::<Diagnostic>(&mut conn)
            .optional()?;
        let diag = if let Some(diag) = existing {
            diag
        } else {
            let new_diag = Diagnostic {
                id: Uuid::new_v4().to_string(),
                vessel_id: vessel_id.clone(),
                report_json: "diagnostics placeholder".to_string(),
                created_at: now,
            };
            diesel::insert_into(diagnostics::table)
                .values(&new_diag)
                .execute(&mut conn)?;
            let persisted: Diagnostic = diagnostics::table.find(&new_diag.id).first(&mut conn)?;
            let _ = (&persisted.id, persisted.created_at);
            persisted
        };
        Ok::<DiagnosticsResponse, ServerError>(DiagnosticsResponse {
            vessel_id: diag.vessel_id,
            status: "pending".to_string(),
            report: DiagnosticsReport {
                summary: diag.report_json,
            },
        })
    })
    .await??;

    Ok(HttpResponse::Ok().json(response))
}

#[utoipa::path(
    post,
    path = "/vessel/{id}/refit",
    params(
        ("id" = String, Path, description = "Vessel identifier")
    ),
    request_body = RefitRequest,
    responses(
        (status = 200, description = "Refit status", body = RefitResponse)
    ),
    tag = "vessel"
)]
#[post("/api/vessel/{id}/refit")]
/// Trigger and persist a refit operation for a vessel in PostgreSQL.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
/// * `path` - Vessel identifier path parameter.
/// * `payload` - Refit request options.
pub async fn vessel_refit(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    payload: web::Json<RefitRequest>,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let vessel_id = path.into_inner();
    let apply = payload.into_inner().apply;
    let pool = state.pool.clone();
    let response = web::block(move || {
        let mut conn = pool.get()?;
        let now = Utc::now().naive_utc();
        ensure_vessel_exists(&mut conn, &vessel_id, now)?;
        let status = if apply {
            "refit applied"
        } else {
            "refit dry-run queued"
        };
        let new_refit = Refit {
            id: Uuid::new_v4().to_string(),
            vessel_id: vessel_id.clone(),
            status: status.to_string(),
            applied: apply,
            output: Some(status.to_string()),
            created_at: now,
        };
        diesel::insert_into(refits::table)
            .values(&new_refit)
            .execute(&mut conn)?;
        let persisted: Refit = refits::table.find(&new_refit.id).first(&mut conn)?;
        let _ = (
            persisted.id,
            persisted.applied,
            persisted.output,
            persisted.created_at,
        );
        Ok::<RefitResponse, ServerError>(RefitResponse {
            vessel_id: persisted.vessel_id,
            status: persisted.status,
        })
    })
    .await??;

    Ok(HttpResponse::Ok().json(response))
}

#[utoipa::path(
    post,
    path = "/vessel/{id}/workflow",
    params(
        ("id" = String, Path, description = "Vessel identifier")
    ),
    request_body = WorkflowRequest,
    responses(
        (status = 200, description = "Workflow result", body = WorkflowResult),
        (status = 500, description = "Workflow failed", body = ErrorResponse)
    ),
    tag = "vessel"
)]
#[post("/api/vessel/{id}/workflow")]
/// Run the GitHub PR + GitLab mirror workflow for a vessel.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
/// * `path` - Vessel identifier path parameter.
/// * `payload` - Workflow execution request.
pub async fn vessel_workflow(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    payload: web::Json<WorkflowRequest>,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let vessel_id = path.into_inner();
    let request = payload.into_inner();
    let pool = state.pool.clone();
    let workflow = state.workflow.clone();
    let response = web::block(move || workflow.run(&pool, &vessel_id, &request)).await??;
    Ok(HttpResponse::Ok().json(response))
}

#[utoipa::path(
    post,
    path = "/voyage/launch",
    request_body = LaunchRequest,
    responses(
        (status = 200, description = "Launch queued", body = LaunchResponse)
    ),
    tag = "voyage"
)]
#[post("/api/voyage/launch")]
/// Launch a voyage pipeline and persist a `Launch` record in PostgreSQL.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `req` - Incoming HTTP request.
/// * `payload` - Launch request containing `voyage_id`.
pub async fn voyage_launch(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<LaunchRequest>,
) -> ServerResult<HttpResponse> {
    let _context = require_auth(&state, &req).await?;
    let voyage_id = payload.into_inner().voyage_id;
    let pool = state.pool.clone();
    let response = web::block(move || {
        let mut conn = pool.get()?;
        let now = Utc::now().naive_utc();
        ensure_voyage_exists(&mut conn, &voyage_id, now)?;
        let new_launch = Launch {
            id: Uuid::new_v4().to_string(),
            voyage_id: voyage_id.clone(),
            status: "launch queued".to_string(),
            created_at: now,
        };
        diesel::insert_into(launches::table)
            .values(&new_launch)
            .execute(&mut conn)?;
        let persisted: Launch = launches::table.find(&new_launch.id).first(&mut conn)?;
        let _ = (persisted.id, persisted.created_at);
        Ok::<LaunchResponse, ServerError>(LaunchResponse {
            voyage_id: persisted.voyage_id,
            status: persisted.status,
        })
    })
    .await??;

    Ok(HttpResponse::Ok().json(response))
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    responses(
        (status = 200, description = "OpenAPI document", body = serde_json::Value)
    ),
    tag = "system"
)]
#[get("/api/openapi.json")]
/// Serve the OpenAPI document.
pub async fn openapi_json() -> ServerResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(ApiDoc::openapi()))
}

/// Ensure a [`Voyage`] row with `voyage_id` exists in PostgreSQL before inserting child records.
///
/// # Arguments
/// * `conn` - Active PostgreSQL connection.
/// * `voyage_id` - Voyage identifier.
/// * `now` - Current UTC timestamp.
fn ensure_voyage_exists(
    conn: &mut diesel::pg::PgConnection,
    voyage_id: &str,
    now: chrono::NaiveDateTime,
) -> ServerResult<()> {
    let exists = voyages::table
        .find(voyage_id)
        .first::<Voyage>(conn)
        .optional()?;
    if exists.is_none() {
        let voyage = Voyage {
            id: voyage_id.to_string(),
            status: "created".to_string(),
            created_at: now,
        };
        diesel::insert_into(voyages::table)
            .values(&voyage)
            .execute(conn)?;
    }
    Ok(())
}

/// Ensure a [`Vessel`] row with `vessel_id` exists in PostgreSQL before inserting child records.
///
/// # Arguments
/// * `conn` - Active PostgreSQL connection.
/// * `vessel_id` - Vessel identifier.
/// * `now` - Current UTC timestamp.
fn ensure_vessel_exists(
    conn: &mut diesel::pg::PgConnection,
    vessel_id: &str,
    now: chrono::NaiveDateTime,
) -> ServerResult<()> {
    let exists = vessels::table
        .find(vessel_id)
        .first::<Vessel>(conn)
        .optional()?;
    if exists.is_none() {
        let default_voyage_id = format!("voyage-{vessel_id}");
        ensure_voyage_exists(conn, &default_voyage_id, now)?;
        let vessel = Vessel {
            id: vessel_id.to_string(),
            voyage_id: default_voyage_id,
            repo_url: None,
            local_path: None,
            created_at: now,
        };
        diesel::insert_into(vessels::table)
            .values(&vessel)
            .execute(conn)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test};
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;

    use crate::crypto::TokenCipher;
    use crate::db::TestDatabase;
    use crate::models::{WorkflowRecord, WorkflowStepRecord};
    use crate::schema::workflows;
    use crate::workflows::WorkflowService;

    /// Base64 test key for encrypting OAuth tokens in route tests.
    const TEST_TOKEN_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

    /// Build a test [`AuthConfig`] pointing at `base_url` and `api_url`.
    fn test_auth_config(base_url: &str, api_url: &str) -> AuthConfig {
        AuthConfig {
            github_client_id: "client-id".to_string(),
            github_client_secret: "client-secret".to_string(),
            github_oauth_url: base_url.to_string(),
            github_api_url: api_url.to_string(),
            github_user_agent: "shipshape-tests".to_string(),
            ui_base_url: "http://127.0.0.1:4200".to_string(),
            github_scopes: vec!["read:user".to_string(), "repo".to_string()],
        }
    }

    /// Test application state and isolated database schema guard.
    struct TestApp {
        /// Shared application state.
        state: web::Data<AppState>,
        /// Isolated database schema dropped when the test completes.
        _db: TestDatabase,
    }

    /// Build a [`TestApp`] with the provided [`AuthConfig`].
    fn build_state(auth: AuthConfig) -> ServerResult<TestApp> {
        let mut test_db = TestDatabase::new()?;
        let pool = test_db.pool()?;
        let token_cipher = TokenCipher::from_base64_keys([TEST_TOKEN_KEY])?;
        let state = web::Data::new(AppState {
            pool,
            workflow: WorkflowService::mock(),
            auth,
            token_cipher,
            http_client: Client::new(),
            diff_store: seed_diff_store(),
        });
        Ok(TestApp {
            state,
            _db: test_db,
        })
    }

    /// Build a [`TestApp`] with default GitHub URLs.
    fn test_state() -> ServerResult<TestApp> {
        build_state(test_auth_config(
            "https://github.com/login/oauth",
            "https://api.github.com",
        ))
    }

    /// Seed a test user and [`AuthSession`] into PostgreSQL.
    fn seed_session(pool: &DbPool, token_cipher: &TokenCipher) -> ServerResult<AuthSession> {
        let mut conn = pool.get()?;
        let now = Utc::now().naive_utc();
        let user = NewUser {
            id: "usr-1".to_string(),
            github_id: "42".to_string(),
            github_login: "pilot".to_string(),
            created_at: now,
        };
        diesel::insert_into(users::table)
            .values(&user)
            .execute(&mut conn)?;
        let session = NewAuthSession {
            id: "sess-1".to_string(),
            user_id: user.id,
            shipshape_token: "token-123".to_string(),
            github_token: token_cipher.encrypt("gh-token")?,
            created_at: now,
            last_used_at: now,
        };
        diesel::insert_into(auth_sessions::table)
            .values(&session)
            .execute(&mut conn)?;
        Ok(AuthSession {
            id: session.id,
            user_id: session.user_id,
            shipshape_token: session.shipshape_token,
            github_token: session.github_token,
            created_at: session.created_at,
            last_used_at: session.last_used_at,
        })
    }

    /// Build an `Authorization: Bearer <token>` header pair for `session`.
    fn auth_header(session: &AuthSession) -> (String, String) {
        (
            "Authorization".to_string(),
            format!("Bearer {}", session.shipshape_token),
        )
    }

    #[actix_web::test]
    async fn auth_config_from_env_loads_defaults() {
        let cfg = AuthConfig::from_env();
        assert!(!cfg.github_scopes.is_empty());
        assert!(cfg.authorize_url().ends_with("/authorize"));
    }

    #[actix_web::test]
    async fn auth_endpoints_handle_login_reuse_and_errors() -> ServerResult<()> {
        let server = MockServer::start();
        let test_app = build_state(test_auth_config(&server.url("/oauth"), &server.url("/api")))?;
        let state = test_app.state.clone();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(configure_routes),
        )
        .await;

        // 1. /api/auth/config
        let req = test::TestRequest::get()
            .uri("/api/auth/config")
            .to_request();
        let cfg_resp: AuthConfigResponse = test::call_and_read_body_json(&app, req).await;
        assert_eq!(cfg_resp.client_id, "client-id");
        assert!(cfg_resp.authorize_url.contains("authorize"));

        // 2. /api/auth/github creates user & session
        let mut token_mock = server.mock(|when, then| {
            when.method(POST).path("/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"access_token":"gh-token"}"#);
        });
        let mut user_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/user")
                .header("authorization", "Bearer gh-token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":123,"login":"octo"}"#);
        });

        let payload = AuthGithubRequest {
            code: "code-123".to_string(),
            redirect_uri: None,
        };
        let req = test::TestRequest::post()
            .uri("/api/auth/github")
            .set_json(&payload)
            .to_request();
        let resp: AuthGithubResponse = test::call_and_read_body_json(&app, req).await;
        token_mock.assert();
        user_mock.assert();
        assert_eq!(resp.user.login, "octo");

        // Second login reuses existing user record
        let req_reuse = test::TestRequest::post()
            .uri("/api/auth/github")
            .set_json(&payload)
            .to_request();
        let resp_reuse: AuthGithubResponse = test::call_and_read_body_json(&app, req_reuse).await;
        assert_eq!(resp_reuse.user.id, resp.user.id);
        token_mock.delete();
        user_mock.delete();

        // 3. /api/auth/github/token creates session & handles error
        let mut dev_user_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/user")
                .header("authorization", "Bearer dev-token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":555,"login":"device"}"#);
        });
        let tok_req = test::TestRequest::post()
            .uri("/api/auth/github/token")
            .set_json(&AuthGithubTokenRequest {
                access_token: "dev-token".to_string(),
            })
            .to_request();
        let tok_resp: AuthGithubResponse = test::call_and_read_body_json(&app, tok_req).await;
        dev_user_mock.assert();
        assert_eq!(tok_resp.user.login, "device");
        dev_user_mock.delete();

        // Verify stored token is encrypted
        let mut conn = state.pool.get()?;
        let stored: AuthSession = auth_sessions::table.first(&mut conn)?;
        assert_eq!(
            state.token_cipher.decrypt(&stored.github_token)?,
            "gh-token"
        );

        // Error in user lookup & token exchange
        let mut err_user = server.mock(|when, then| {
            when.method(GET).path("/api/user");
            then.status(500).body("boom");
        });
        let fail_tok_req = test::TestRequest::post()
            .uri("/api/auth/github/token")
            .set_json(&AuthGithubTokenRequest {
                access_token: "dev-token".to_string(),
            })
            .to_request();
        let fail_tok_resp = test::call_service(&app, fail_tok_req).await;
        assert_eq!(fail_tok_resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        err_user.delete();

        let mut err_oauth = server.mock(|when, then| {
            when.method(POST).path("/oauth/access_token");
            then.status(500).body("boom");
        });
        let fail_oauth_req = test::TestRequest::post()
            .uri("/api/auth/github")
            .set_json(&payload)
            .to_request();
        let fail_oauth_resp = test::call_service(&app, fail_oauth_req).await;
        assert_eq!(fail_oauth_resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        err_oauth.delete();

        // 4. /api/auth/me: missing header, invalid scheme, unknown token, and valid session
        let no_hdr = test::TestRequest::get().uri("/api/auth/me").to_request();
        assert_eq!(
            test::call_service(&app, no_hdr).await.status(),
            StatusCode::UNAUTHORIZED
        );

        let bad_hdr = test::TestRequest::get()
            .uri("/api/auth/me")
            .insert_header(("Authorization", "Token abc"))
            .to_request();
        assert_eq!(
            test::call_service(&app, bad_hdr).await.status(),
            StatusCode::UNAUTHORIZED
        );

        let missing_tok = test::TestRequest::get()
            .uri("/api/auth/me")
            .insert_header(("Authorization", "Bearer unknown"))
            .to_request();
        assert_eq!(
            test::call_service(&app, missing_tok).await.status(),
            StatusCode::UNAUTHORIZED
        );

        let ok_me = test::TestRequest::get()
            .uri("/api/auth/me")
            .insert_header(("Authorization", format!("Bearer {}", resp.token)))
            .to_request();
        let me_user: AuthUser = test::call_and_read_body_json(&app, ok_me).await;
        assert_eq!(me_user.login, "octo");
        Ok(())
    }

    #[actix_web::test]
    async fn fleet_voyage_vessel_and_diff_endpoints_persist_and_respond() -> ServerResult<()> {
        let test_app = test_state()?;
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher)?;
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(configure_routes),
        )
        .await;

        // 1. Board a voyage -> inserts into voyages and vessels (plus empty repo list branch)
        let empty_board_req = test::TestRequest::post()
            .uri("/api/voyage/board")
            .insert_header(auth_header(&session))
            .set_json(&VoyageBoardRequest { repos: Vec::new() })
            .to_request();
        let _: VoyageBoardResponse = test::call_and_read_body_json(&app, empty_board_req).await;

        let board_req = test::TestRequest::post()
            .uri("/api/voyage/board")
            .insert_header(auth_header(&session))
            .set_json(&VoyageBoardRequest {
                repos: vec!["https://example.com/repo.git".to_string()],
            })
            .to_request();
        let board_resp: VoyageBoardResponse = test::call_and_read_body_json(&app, board_req).await;
        assert_eq!(board_resp.status, "queued 1 repos");

        // 2. Dashboard reflects boarded vessel
        let dash_req = test::TestRequest::get()
            .uri("/api/dashboard")
            .insert_header(auth_header(&session))
            .to_request();
        let dash_resp: DashboardResponse = test::call_and_read_body_json(&app, dash_req).await;
        assert_eq!(dash_resp.metrics.len(), 4);
        assert_eq!(dash_resp.vessels.len(), 4);

        // 3. Batch runs reflects boarded voyages (4 seed + 2 boarded)
        let batch_req = test::TestRequest::get()
            .uri("/api/batch/runs")
            .insert_header(auth_header(&session))
            .to_request();
        let batch_resp: BatchRunsResponse = test::call_and_read_body_json(&app, batch_req).await;
        assert_eq!(batch_resp.runs.len(), 6);

        // 4. Diffs list & update (valid, empty path 400, missing path 404)
        let diffs_req = test::TestRequest::get()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .to_request();
        let diffs_resp: DiffListingResponse = test::call_and_read_body_json(&app, diffs_req).await;
        assert_eq!(diffs_resp.files.len(), 3);

        let upd_req = test::TestRequest::post()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .set_json(&DiffUpdateRequest {
                path: "src/inspector.rs".to_string(),
                modified: "updated content".to_string(),
            })
            .to_request();
        let upd_resp: DiffUpdateResponse = test::call_and_read_body_json(&app, upd_req).await;
        assert_eq!(upd_resp.file.modified, "updated content");

        let empty_diff_req = test::TestRequest::post()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .set_json(&DiffUpdateRequest {
                path: "   ".to_string(),
                modified: "noop".to_string(),
            })
            .to_request();
        assert_eq!(
            test::call_service(&app, empty_diff_req).await.status(),
            StatusCode::BAD_REQUEST
        );

        let missing_diff_req = test::TestRequest::post()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .set_json(&DiffUpdateRequest {
                path: "missing.rs".to_string(),
                modified: "noop".to_string(),
            })
            .to_request();
        assert_eq!(
            test::call_service(&app, missing_diff_req).await.status(),
            StatusCode::NOT_FOUND
        );

        // 5. Control options & queue
        let ctrl_opt_req = test::TestRequest::get()
            .uri("/api/control/options")
            .insert_header(auth_header(&session))
            .to_request();
        let ctrl_opt_resp: ControlOptionsResponse =
            test::call_and_read_body_json(&app, ctrl_opt_req).await;
        assert_eq!(ctrl_opt_resp.mechanics.len(), 4);

        let ctrl_q_req = test::TestRequest::post()
            .uri("/api/control/queue")
            .insert_header(auth_header(&session))
            .set_json(&ControlQueueRequest {
                source_type: "url".to_string(),
                source_value: "https://github.com/shipshape/fleet-core".to_string(),
                mode: "audit".to_string(),
                dry_run: true,
                mechanic_ids: vec!["cpp-types".to_string(), "ci-drydock".to_string()],
            })
            .to_request();
        let ctrl_q_resp: ControlQueueResponse =
            test::call_and_read_body_json(&app, ctrl_q_req).await;
        assert!(ctrl_q_resp.log.detail.contains("2 mechanics"));

        // 6. Vessel diagnostics (first creates row, second reads existing row)
        for _ in 0..2 {
            let diag_req = test::TestRequest::get()
                .uri("/api/vessel/v-123/diagnostics")
                .insert_header(auth_header(&session))
                .to_request();
            let diag_resp: DiagnosticsResponse =
                test::call_and_read_body_json(&app, diag_req).await;
            assert_eq!(diag_resp.vessel_id, "v-123");
            assert_eq!(diag_resp.report.summary, "diagnostics placeholder");
        }

        // 7. Vessel refit (apply = true and apply = false)
        for (apply, expected) in [(true, "refit applied"), (false, "refit dry-run queued")] {
            let refit_req = test::TestRequest::post()
                .uri("/api/vessel/v-123/refit")
                .insert_header(auth_header(&session))
                .set_json(&RefitRequest { apply })
                .to_request();
            let refit_resp: RefitResponse = test::call_and_read_body_json(&app, refit_req).await;
            assert_eq!(refit_resp.status, expected);
        }

        // 8. Vessel workflow & verify WorkflowRecord + WorkflowStepRecord in DB
        let wf_req = test::TestRequest::post()
            .uri("/api/vessel/v-123/workflow")
            .insert_header(auth_header(&session))
            .set_json(&WorkflowRequest {
                repo: crate::workflows::RepoSpec {
                    repo_url: "https://github.com/shipshape/shipshape-demo.git".to_string(),
                    base_branch: "main".to_string(),
                    local_path: None,
                },
                patch: crate::workflows::PatchSpec {
                    diff: "diff --git a/README.md b/README.md
"
                    .to_string(),
                    branch: "shipshape/refit".to_string(),
                    commit_message: "Apply ShipShape refit".to_string(),
                },
                pr: crate::workflows::PullRequestSpec {
                    title: "ShipShape refit".to_string(),
                    body: None,
                    draft: false,
                },
                fleet_report: None,
                gitlab: crate::workflows::GitLabSpec {
                    mirror_url: "https://gitlab.example.com/shipshape/shipshape-demo.git"
                        .to_string(),
                    project_path: "shipshape/shipshape-demo".to_string(),
                    pipeline_ref: None,
                },
            })
            .to_request();
        let wf_resp: WorkflowResult = test::call_and_read_body_json(&app, wf_req).await;
        assert_eq!(wf_resp.steps.len(), 7);

        let mut conn = state.pool.get()?;
        let wf_record: WorkflowRecord = workflows::table
            .find(&wf_resp.workflow_id)
            .first(&mut conn)?;
        assert_eq!(wf_record.vessel_id, "v-123");
        assert_eq!(wf_record.status, "success");
        assert!(wf_record.pr_url.is_some());
        assert!(wf_record.pipeline_url.is_some());
        let _wf_ts = wf_record.created_at;

        let step_records: Vec<WorkflowStepRecord> =
            WorkflowStepRecord::belonging_to(&wf_record).load::<WorkflowStepRecord>(&mut conn)?;
        assert_eq!(step_records.len(), 7);
        assert_eq!(step_records[0].workflow_id, wf_record.id);
        assert_eq!(step_records[0].kind, "apply_patch");
        assert_eq!(step_records[0].status, "success");
        assert!(step_records[0].detail.is_some());
        let _step_ts = step_records[0].created_at;

        // 9. Voyage launch (first creates missing voyage, second reuses existing voyage)
        for _ in 0..2 {
            let launch_req = test::TestRequest::post()
                .uri("/api/voyage/launch")
                .insert_header(auth_header(&session))
                .set_json(&LaunchRequest {
                    voyage_id: "voyage-new".to_string(),
                })
                .to_request();
            let launch_resp: LaunchResponse = test::call_and_read_body_json(&app, launch_req).await;
            assert_eq!(launch_resp.voyage_id, "voyage-new");
            assert_eq!(launch_resp.status, "launch queued");
        }

        // 10. OpenAPI JSON
        let oa_req = test::TestRequest::get()
            .uri("/api/openapi.json")
            .to_request();
        let oa_resp: serde_json::Value = test::call_and_read_body_json(&app, oa_req).await;
        assert!(oa_resp.get("openapi").is_some());
        Ok(())
    }
}
