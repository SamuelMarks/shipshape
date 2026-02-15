//! HTTP handlers for ShipShape server.

use actix_web::{HttpRequest, HttpResponse, Responder, get, post, web};
use chrono::Utc;
use diesel::prelude::*;
use diesel::result::OptionalExtension;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::crypto::TokenCipher;
use crate::db::DbPool;
use crate::models::{AuthSession, NewAuthSession, NewUser, User};
use crate::openapi::ApiDoc;
use crate::schema::{auth_sessions, users};
use crate::workflows::{WorkflowRequest, WorkflowService};

#[derive(Clone)]
/// Shared application state for handlers.
pub struct AppState {
    /// Database connection pool.
    pub pool: DbPool,
    /// Workflow orchestration service.
    pub workflow: WorkflowService,
    /// Authentication configuration.
    pub auth: AuthConfig,
    /// Token encryption helper for storing OAuth secrets.
    pub token_cipher: TokenCipher,
    /// In-memory diff listing and edits.
    pub diff_store: Arc<RwLock<Vec<DiffFile>>>,
}

/// Authentication configuration loaded from the environment.
#[derive(Clone)]
pub struct AuthConfig {
    /// GitHub OAuth client id.
    pub github_client_id: String,
    /// GitHub OAuth client secret.
    pub github_client_secret: String,
    /// GitHub OAuth base URL.
    pub github_oauth_url: String,
    /// GitHub API base URL.
    pub github_api_url: String,
    /// GitHub API user agent.
    pub github_user_agent: String,
    /// UI base URL for OAuth redirect.
    pub ui_base_url: String,
    /// GitHub OAuth scopes.
    pub github_scopes: Vec<String>,
}

impl AuthConfig {
    /// Build auth config from environment variables.
    #[cfg_attr(test, allow(dead_code))]
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

    fn authorize_url(&self) -> String {
        format!("{}/authorize", self.github_oauth_url.trim_end_matches('/'))
    }

    fn token_url(&self) -> String {
        format!(
            "{}/access_token",
            self.github_oauth_url.trim_end_matches('/')
        )
    }

    fn redirect_uri(&self) -> String {
        format!("{}/auth/callback", self.ui_base_url.trim_end_matches('/'))
    }
}

#[derive(Clone)]
struct AuthContext {
    user: User,
}

/// OAuth configuration shared with the UI.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigResponse {
    /// GitHub OAuth client id.
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
    /// GitHub user id.
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

/// Seed the in-memory diff store with placeholder content.
pub fn seed_diff_store() -> Arc<RwLock<Vec<DiffFile>>> {
    Arc::new(RwLock::new(default_diff_files()))
}

fn default_diff_files() -> Vec<DiffFile> {
    vec![
        DiffFile {
            path: "src/inspector.rs".to_string(),
            summary: "Added health score heuristics for coverage risk.".to_string(),
            language: "rust".to_string(),
            original: "pub fn health_score(report: &FleetReport) -> u8 {\\n  0\\n}\\n"
                .to_string(),
            modified: "pub fn health_score(report: &FleetReport) -> u8 {\\n  let mut score = 70;\\n  if report.coverage.low_count > 0 {\\n    score = score.saturating_sub(20);\\n  }\\n  score\\n}\\n"
                .to_string(),
            tone: "good".to_string(),
            status_label: "Modified".to_string(),
        },
        DiffFile {
            path: "src/drydock.rs".to_string(),
            summary: "Added CMake detection and notebook-only guardrails.".to_string(),
            language: "rust".to_string(),
            original: "pub fn detect_stack(files: &[String]) -> Stack {\\n  Stack::Unknown\\n}\\n"
                .to_string(),
            modified: "pub fn detect_stack(files: &[String]) -> Stack {\\n  if files.iter().any(|name| name.contains(\"CMakeLists\")) {\\n    return Stack::Cmake;\\n  }\\n  Stack::Unknown\\n}\\n"
                .to_string(),
            tone: "warn".to_string(),
            status_label: "Modified".to_string(),
        },
        DiffFile {
            path: "src/pr_template.rs".to_string(),
            summary: "Interpolated ShipShape stats into PR templates.".to_string(),
            language: "rust".to_string(),
            original: "const SHIPSHAPE_STATS: &str = \"{{SHIPSHAPE_STATS}}\";\\n"
                .to_string(),
            modified: "const SHIPSHAPE_STATS: &str = \"{{SHIPSHAPE_STATS}}\";\\nconst SHIPSHAPE_FIXES: &str = \"{{SHIPSHAPE_FIXES}}\";\\n"
                .to_string(),
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
    /// Selected mechanic ids.
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

#[derive(Serialize)]
struct GithubTokenRequest {
    client_id: String,
    client_secret: String,
    code: String,
    redirect_uri: Option<String>,
}

#[derive(Deserialize)]
struct GithubTokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct GithubUserResponse {
    id: u64,
    login: String,
}

fn unauthorized(message: &str) -> HttpResponse {
    HttpResponse::Unauthorized().json(ErrorResponse {
        message: message.to_string(),
    })
}

fn extract_bearer_token(req: &HttpRequest) -> Result<String, HttpResponse> {
    let header = req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| unauthorized("missing authorization header"))?;
    let token = header
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| unauthorized("missing bearer token"))?;
    Ok(token.to_string())
}

async fn require_auth(
    state: &web::Data<AppState>,
    req: &HttpRequest,
) -> Result<AuthContext, HttpResponse> {
    let token = extract_bearer_token(req)?;
    let pool = state.pool.clone();
    let result = web::block(move || {
        let mut conn = pool.get().map_err(|err| err.to_string())?;
        let session = auth_sessions::table
            .filter(auth_sessions::shipshape_token.eq(&token))
            .first::<AuthSession>(&mut conn)
            .optional()
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "session not found".to_string())?;
        let user = users::table
            .filter(users::id.eq(&session.user_id))
            .first::<User>(&mut conn)
            .map_err(|err| err.to_string())?;
        Ok::<AuthContext, String>(AuthContext { user })
    })
    .await;

    match result {
        Ok(Ok(context)) => Ok(context),
        _ => Err(unauthorized("invalid session")),
    }
}

fn exchange_github_code(
    client: &Client,
    auth: &AuthConfig,
    payload: &AuthGithubRequest,
) -> Result<String, String> {
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
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub token exchange failed: {}",
            response.status()
        ));
    }
    let body: GithubTokenResponse = response.json().map_err(|err| err.to_string())?;
    Ok(body.access_token)
}

fn fetch_github_user(
    client: &Client,
    auth: &AuthConfig,
    token: &str,
) -> Result<GithubUserResponse, String> {
    let response = client
        .get(format!(
            "{}/user",
            auth.github_api_url.trim_end_matches('/')
        ))
        .header("User-Agent", auth.github_user_agent.clone())
        .bearer_auth(token)
        .send()
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("GitHub user lookup failed: {}", response.status()));
    }
    response.json().map_err(|err| err.to_string())
}

/// Persist a ShipShape auth session from a GitHub profile and token.
fn persist_auth_session(
    conn: &mut diesel::pg::PgConnection,
    token_cipher: &TokenCipher,
    github_user: GithubUserResponse,
    github_token: String,
) -> Result<AuthGithubResponse, String> {
    let now = Utc::now().naive_utc();
    let existing = users::table
        .filter(users::github_id.eq(github_user.id.to_string()))
        .first::<User>(conn)
        .optional()
        .map_err(|err| err.to_string())?;
    let user = if let Some(user) = existing {
        user
    } else {
        let new_user = NewUser {
            id: Uuid::new_v4().to_string(),
            github_id: github_user.id.to_string(),
            github_login: github_user.login.clone(),
            created_at: now,
        };
        diesel::insert_into(users::table)
            .values(&new_user)
            .execute(conn)
            .map_err(|err| err.to_string())?;
        User {
            id: new_user.id,
            github_id: new_user.github_id,
            github_login: new_user.github_login,
            created_at: new_user.created_at,
        }
    };
    let shipshape_token = Uuid::new_v4().to_string();
    let encrypted_github_token = token_cipher.encrypt(&github_token);
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
        .execute(conn)
        .map_err(|err| err.to_string())?;
    Ok(AuthGithubResponse {
        token: shipshape_token,
        user: AuthUser {
            id: user.id,
            login: user.github_login,
            github_id: user.github_id,
        },
    })
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
pub async fn auth_config(state: web::Data<AppState>) -> impl Responder {
    let config = &state.auth;
    HttpResponse::Ok().json(AuthConfigResponse {
        client_id: config.github_client_id.clone(),
        authorize_url: config.authorize_url(),
        scopes: config.github_scopes.clone(),
        redirect_uri: config.redirect_uri(),
    })
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
pub async fn auth_github(
    state: web::Data<AppState>,
    payload: web::Json<AuthGithubRequest>,
) -> impl Responder {
    let pool = state.pool.clone();
    let auth = state.auth.clone();
    let token_cipher = state.token_cipher.clone();
    let payload = payload.into_inner();
    let result = web::block(move || {
        let client = Client::new();
        let github_token = exchange_github_code(&client, &auth, &payload)?;
        let github_user = fetch_github_user(&client, &auth, &github_token)?;
        let mut conn = pool.get().map_err(|err| err.to_string())?;
        persist_auth_session(&mut conn, &token_cipher, github_user, github_token)
    })
    .await
    .unwrap_or_else(|err| Err(format!("auth exchange failed: {err}")));

    match result {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(message) => HttpResponse::InternalServerError().json(ErrorResponse { message }),
    }
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
pub async fn auth_github_token(
    state: web::Data<AppState>,
    payload: web::Json<AuthGithubTokenRequest>,
) -> impl Responder {
    let pool = state.pool.clone();
    let auth = state.auth.clone();
    let token_cipher = state.token_cipher.clone();
    let payload = payload.into_inner();
    let result = web::block(move || {
        let client = Client::new();
        let github_user = fetch_github_user(&client, &auth, &payload.access_token)?;
        let mut conn = pool.get().map_err(|err| err.to_string())?;
        persist_auth_session(&mut conn, &token_cipher, github_user, payload.access_token)
    })
    .await
    .unwrap_or_else(|err| Err(format!("auth exchange failed: {err}")));

    match result {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(message) => HttpResponse::InternalServerError().json(ErrorResponse { message }),
    }
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
pub async fn auth_me(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    let context = match require_auth(&state, &req).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    HttpResponse::Ok().json(AuthUser {
        id: context.user.id,
        login: context.user.github_login,
        github_id: context.user.github_id,
    })
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
/// Fetch dashboard data.
pub async fn dashboard(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    let metrics = vec![
        FleetMetric {
            label: "Active vessels".to_string(),
            value: "128".to_string(),
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
    let vessels = vec![
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
    HttpResponse::Ok().json(DashboardResponse {
        metrics,
        vessels,
        alerts,
    })
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
/// Fetch batch runs.
pub async fn batch_runs(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    let runs = vec![
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
    HttpResponse::Ok().json(BatchRunsResponse { runs })
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
pub async fn diffs(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    let files = match state.diff_store.read() {
        Ok(store) => store.clone(),
        Err(_) => {
            return HttpResponse::InternalServerError().json(ErrorResponse {
                message: "diff store unavailable".to_string(),
            });
        }
    };
    HttpResponse::Ok().json(DiffListingResponse { files })
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
pub async fn diff_update(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<DiffUpdateRequest>,
) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    if payload.path.trim().is_empty() {
        return HttpResponse::BadRequest().json(ErrorResponse {
            message: "diff path is required".to_string(),
        });
    }
    let mut store = match state.diff_store.write() {
        Ok(store) => store,
        Err(_) => {
            return HttpResponse::InternalServerError().json(ErrorResponse {
                message: "diff store unavailable".to_string(),
            });
        }
    };
    if let Some(entry) = store.iter_mut().find(|file| file.path == payload.path) {
        entry.modified = payload.modified.clone();
        return HttpResponse::Ok().json(DiffUpdateResponse {
            file: entry.clone(),
        });
    }
    HttpResponse::NotFound().json(ErrorResponse {
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
pub async fn control_options(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
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
    HttpResponse::Ok().json(ControlOptionsResponse {
        mechanics,
        activity,
    })
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
/// Queue a control room run.
pub async fn control_queue(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<ControlQueueRequest>,
) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
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
    HttpResponse::Ok().json(ControlQueueResponse {
        run_id: Uuid::new_v4().to_string(),
        log,
    })
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
/// Queue a voyage for the provided repositories.
pub async fn voyage_board(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<VoyageBoardRequest>,
) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    let voyage_id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let response = VoyageBoardResponse {
        voyage_id,
        status: format!("queued {} repos", payload.repos.len()),
        created_at,
    };
    HttpResponse::Ok().json(response)
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
/// Fetch diagnostics for a vessel.
pub async fn vessel_diagnostics(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    let vessel_id = path.into_inner();
    let response = DiagnosticsResponse {
        vessel_id,
        status: "pending".to_string(),
        report: DiagnosticsReport {
            summary: "diagnostics placeholder".to_string(),
        },
    };
    HttpResponse::Ok().json(response)
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
/// Trigger a refit operation for a vessel.
pub async fn vessel_refit(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    payload: web::Json<RefitRequest>,
) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    let vessel_id = path.into_inner();
    let status = if payload.apply {
        "refit applied"
    } else {
        "refit dry-run queued"
    };
    let response = RefitResponse {
        vessel_id,
        status: status.to_string(),
    };
    HttpResponse::Ok().json(response)
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
pub async fn vessel_workflow(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    payload: web::Json<WorkflowRequest>,
) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    let vessel_id = path.into_inner();
    let request = payload.into_inner();
    let pool = state.pool.clone();
    let workflow = state.workflow.clone();
    let result = web::block(move || workflow.run(&pool, &vessel_id, &request)).await;

    match result {
        Ok(Ok(response)) => HttpResponse::Ok().json(response),
        Ok(Err(err)) => HttpResponse::InternalServerError().json(ErrorResponse {
            message: err.to_string(),
        }),
        Err(err) => HttpResponse::InternalServerError().json(ErrorResponse {
            message: format!("workflow task failed: {err}"),
        }),
    }
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
/// Launch a voyage pipeline.
pub async fn voyage_launch(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Json<LaunchRequest>,
) -> impl Responder {
    if let Err(response) = require_auth(&state, &req).await {
        return response;
    }
    let response = LaunchResponse {
        voyage_id: payload.voyage_id.clone(),
        status: "launch queued".to_string(),
    };
    HttpResponse::Ok().json(response)
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
pub async fn openapi_json() -> impl Responder {
    HttpResponse::Ok().json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test};
    use httpmock::Method::GET;
    use httpmock::Method::POST;
    use httpmock::MockServer;

    use crate::crypto::TokenCipher;
    use crate::db::TestDatabase;
    use crate::workflows::WorkflowResult;
    use crate::workflows::WorkflowService;

    const TEST_TOKEN_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

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

    struct TestApp {
        state: web::Data<AppState>,
        _db: TestDatabase,
    }

    fn build_state(auth: AuthConfig) -> TestApp {
        let mut test_db = TestDatabase::new();
        let pool = test_db.pool();
        let token_cipher = TokenCipher::from_base64_keys([TEST_TOKEN_KEY]).expect("token cipher");
        let state = web::Data::new(AppState {
            pool,
            workflow: WorkflowService::mock(),
            auth,
            token_cipher,
            diff_store: seed_diff_store(),
        });
        TestApp {
            state,
            _db: test_db,
        }
    }

    fn test_state() -> TestApp {
        build_state(test_auth_config(
            "https://github.com/login/oauth",
            "https://api.github.com",
        ))
    }

    fn seed_session(pool: &DbPool, token_cipher: &TokenCipher) -> AuthSession {
        let mut conn = pool.get().expect("conn");
        let now = Utc::now().naive_utc();
        let user = NewUser {
            id: "usr-1".to_string(),
            github_id: "42".to_string(),
            github_login: "pilot".to_string(),
            created_at: now,
        };
        diesel::insert_into(users::table)
            .values(&user)
            .execute(&mut conn)
            .expect("insert user");
        let session = NewAuthSession {
            id: "sess-1".to_string(),
            user_id: user.id.clone(),
            shipshape_token: "token-123".to_string(),
            github_token: token_cipher.encrypt("gh-token"),
            created_at: now,
            last_used_at: now,
        };
        diesel::insert_into(auth_sessions::table)
            .values(&session)
            .execute(&mut conn)
            .expect("insert session");
        AuthSession {
            id: session.id,
            user_id: session.user_id,
            shipshape_token: session.shipshape_token,
            github_token: session.github_token,
            created_at: session.created_at,
            last_used_at: session.last_used_at,
        }
    }

    fn auth_header(session: &AuthSession) -> (String, String) {
        (
            "Authorization".to_string(),
            format!("Bearer {}", session.shipshape_token),
        )
    }

    #[actix_web::test]
    async fn auth_config_returns_payload() {
        let test_app = test_state();
        let app = test::init_service(
            App::new()
                .app_data(test_app.state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/auth/config")
            .to_request();
        let resp: AuthConfigResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.client_id, "client-id");
        assert!(resp.authorize_url.contains("authorize"));
        assert_eq!(resp.scopes.len(), 2);
        assert!(resp.redirect_uri.contains("/auth/callback"));
    }

    #[actix_web::test]
    async fn auth_github_creates_session() {
        let server = MockServer::start();
        let token_mock = server.mock(|when, then| {
            when.method(POST).path("/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"access_token":"gh-token"}"#);
        });
        let user_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/user")
                .header("authorization", "Bearer gh-token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":123,"login":"octo"}"#);
        });

        let test_app = build_state(test_auth_config(&server.url("/oauth"), &server.url("/api")));
        let state = test_app.state.clone();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
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
        assert!(!resp.token.is_empty());

        let mut conn = state.pool.get().expect("conn");
        let stored: AuthSession = auth_sessions::table.first(&mut conn).expect("session");
        assert_ne!(stored.github_token, "gh-token");
        let decrypted = state
            .token_cipher
            .decrypt(&stored.github_token)
            .expect("decrypt");
        assert_eq!(decrypted, "gh-token");
    }

    #[actix_web::test]
    async fn auth_github_token_creates_session() {
        let server = MockServer::start();
        let user_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/user")
                .header("authorization", "Bearer gh-token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":555,"login":"device"}"#);
        });

        let test_app = build_state(test_auth_config(&server.url("/oauth"), &server.url("/api")));
        let state = test_app.state.clone();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = AuthGithubTokenRequest {
            access_token: "gh-token".to_string(),
        };
        let req = test::TestRequest::post()
            .uri("/api/auth/github/token")
            .set_json(&payload)
            .to_request();
        let resp: AuthGithubResponse = test::call_and_read_body_json(&app, req).await;

        user_mock.assert();
        assert_eq!(resp.user.login, "device");
        assert!(!resp.token.is_empty());

        let mut conn = state.pool.get().expect("conn");
        let stored: AuthSession = auth_sessions::table.first(&mut conn).expect("session");
        assert_ne!(stored.github_token, "gh-token");
        let decrypted = state
            .token_cipher
            .decrypt(&stored.github_token)
            .expect("decrypt");
        assert_eq!(decrypted, "gh-token");
    }

    #[actix_web::test]
    async fn auth_github_token_reports_user_lookup_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/api/user");
            then.status(500).body("boom");
        });

        let test_app = build_state(test_auth_config(&server.url("/oauth"), &server.url("/api")));
        let state = test_app.state.clone();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = AuthGithubTokenRequest {
            access_token: "gh-token".to_string(),
        };
        let req = test::TestRequest::post()
            .uri("/api/auth/github/token")
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 500);
    }

    #[actix_web::test]
    async fn auth_github_reuses_user() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"access_token":"gh-token-2"}"#);
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/api/user")
                .header("authorization", "Bearer gh-token-2");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":777,"login":"reuse"}"#);
        });

        let test_app = build_state(test_auth_config(&server.url("/oauth"), &server.url("/api")));
        let state = test_app.state.clone();
        let now = Utc::now().naive_utc();
        {
            let mut conn = state.pool.get().expect("conn");
            let user = NewUser {
                id: "usr-existing".to_string(),
                github_id: "777".to_string(),
                github_login: "reuse".to_string(),
                created_at: now,
            };
            diesel::insert_into(users::table)
                .values(&user)
                .execute(&mut conn)
                .expect("seed user");
        }

        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = AuthGithubRequest {
            code: "code-456".to_string(),
            redirect_uri: None,
        };
        let req = test::TestRequest::post()
            .uri("/api/auth/github")
            .set_json(&payload)
            .to_request();
        let resp: AuthGithubResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.user.github_id, "777");
    }

    #[actix_web::test]
    async fn auth_github_reports_errors() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/oauth/access_token");
            then.status(500).body("boom");
        });

        let test_app = build_state(test_auth_config(&server.url("/oauth"), &server.url("/api")));
        let state = test_app.state.clone();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = AuthGithubRequest {
            code: "code-err".to_string(),
            redirect_uri: None,
        };
        let req = test::TestRequest::post()
            .uri("/api/auth/github")
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 500);
    }

    #[actix_web::test]
    async fn auth_github_reports_user_lookup_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"access_token":"gh-token"}"#);
        });
        server.mock(|when, then| {
            when.method(GET).path("/api/user");
            then.status(500).body("boom");
        });

        let test_app = build_state(test_auth_config(&server.url("/oauth"), &server.url("/api")));
        let state = test_app.state.clone();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = AuthGithubRequest {
            code: "code-err".to_string(),
            redirect_uri: None,
        };
        let req = test::TestRequest::post()
            .uri("/api/auth/github")
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 500);
    }

    #[actix_web::test]
    async fn auth_me_requires_auth() {
        let test_app = test_state();
        let app = test::init_service(
            App::new()
                .app_data(test_app.state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get().uri("/api/auth/me").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 401);
    }

    #[actix_web::test]
    async fn auth_me_rejects_invalid_header() {
        let test_app = test_state();
        let app = test::init_service(
            App::new()
                .app_data(test_app.state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/auth/me")
            .insert_header(("Authorization", "Token abc"))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 401);
    }

    #[actix_web::test]
    async fn auth_me_rejects_unknown_session() {
        let test_app = test_state();
        let app = test::init_service(
            App::new()
                .app_data(test_app.state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/auth/me")
            .insert_header(("Authorization", "Bearer missing"))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 401);
    }

    #[actix_web::test]
    async fn auth_me_returns_user() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/auth/me")
            .insert_header(auth_header(&session))
            .to_request();
        let resp: AuthUser = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.github_id, "42");
    }

    #[actix_web::test]
    async fn dashboard_returns_payload() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/dashboard")
            .insert_header(auth_header(&session))
            .to_request();
        let resp: DashboardResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.metrics.len(), 4);
        assert_eq!(resp.vessels.len(), 3);
    }

    #[actix_web::test]
    async fn batch_runs_returns_payload() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/batch/runs")
            .insert_header(auth_header(&session))
            .to_request();
        let resp: BatchRunsResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.runs.len(), 4);
    }

    #[actix_web::test]
    async fn diffs_returns_payload() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .to_request();
        let resp: DiffListingResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.files.len(), 3);
    }

    #[actix_web::test]
    async fn diff_update_persists_modified_content() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = DiffUpdateRequest {
            path: "src/inspector.rs".to_string(),
            modified: "pub fn health_score(report: &FleetReport) -> u8 {\\n  42\\n}\\n".to_string(),
        };
        let req = test::TestRequest::post()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .set_json(&payload)
            .to_request();
        let resp: DiffUpdateResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.file.path, "src/inspector.rs");
        assert!(resp.file.modified.contains("42"));

        let req = test::TestRequest::get()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .to_request();
        let listing: DiffListingResponse = test::call_and_read_body_json(&app, req).await;
        let updated = listing
            .files
            .iter()
            .find(|file| file.path == "src/inspector.rs")
            .expect("diff file");

        assert_eq!(updated.modified, payload.modified);
    }

    #[actix_web::test]
    async fn diff_update_rejects_empty_path() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = DiffUpdateRequest {
            path: "  ".to_string(),
            modified: "noop".to_string(),
        };
        let req = test::TestRequest::post()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert!(body.message.contains("diff path"));
    }

    #[actix_web::test]
    async fn diff_update_returns_not_found() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = DiffUpdateRequest {
            path: "missing.rs".to_string(),
            modified: "noop".to_string(),
        };
        let req = test::TestRequest::post()
            .uri("/api/diffs")
            .insert_header(auth_header(&session))
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert!(body.message.contains("not found"));
    }

    #[actix_web::test]
    async fn control_options_returns_payload() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/control/options")
            .insert_header(auth_header(&session))
            .to_request();
        let resp: ControlOptionsResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.mechanics.len(), 4);
        assert_eq!(resp.activity.len(), 3);
    }

    #[actix_web::test]
    async fn control_queue_returns_log() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = ControlQueueRequest {
            source_type: "url".to_string(),
            source_value: "https://github.com/shipshape/fleet-core".to_string(),
            mode: "audit".to_string(),
            dry_run: true,
            mechanic_ids: vec!["cpp-types".to_string(), "ci-drydock".to_string()],
        };
        let req = test::TestRequest::post()
            .uri("/api/control/queue")
            .insert_header(auth_header(&session))
            .set_json(&payload)
            .to_request();
        let resp: ControlQueueResponse = test::call_and_read_body_json(&app, req).await;

        assert!(resp.log.detail.contains("2 mechanics"));
    }

    #[actix_web::test]
    async fn voyage_board_returns_payload() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = VoyageBoardRequest {
            repos: vec!["https://example.com/repo.git".to_string()],
        };
        let req = test::TestRequest::post()
            .uri("/api/voyage/board")
            .insert_header(auth_header(&session))
            .set_json(&payload)
            .to_request();
        let resp: VoyageBoardResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.status, "queued 1 repos");
        assert!(!resp.voyage_id.is_empty());
        assert!(!resp.created_at.is_empty());
    }

    #[actix_web::test]
    async fn vessel_diagnostics_returns_report() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/vessel/v-123/diagnostics")
            .insert_header(auth_header(&session))
            .to_request();
        let resp: DiagnosticsResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.vessel_id, "v-123");
        assert_eq!(resp.status, "pending");
        assert_eq!(resp.report.summary, "diagnostics placeholder");
    }

    #[actix_web::test]
    async fn vessel_refit_returns_status() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = RefitRequest { apply: true };
        let req = test::TestRequest::post()
            .uri("/api/vessel/v-123/refit")
            .insert_header(auth_header(&session))
            .set_json(&payload)
            .to_request();
        let resp: RefitResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.vessel_id, "v-123");
        assert_eq!(resp.status, "refit applied");
    }

    #[actix_web::test]
    async fn vessel_workflow_returns_result() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = WorkflowRequest {
            repo: crate::workflows::RepoSpec {
                repo_url: "https://github.com/shipshape/shipshape-demo.git".to_string(),
                base_branch: "main".to_string(),
                local_path: None,
            },
            patch: crate::workflows::PatchSpec {
                diff: "diff --git a/README.md b/README.md\n".to_string(),
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
                mirror_url: "https://gitlab.example.com/shipshape/shipshape-demo.git".to_string(),
                project_path: "shipshape/shipshape-demo".to_string(),
                pipeline_ref: None,
            },
        };
        let req = test::TestRequest::post()
            .uri("/api/vessel/v-456/workflow")
            .insert_header(auth_header(&session))
            .set_json(&payload)
            .to_request();
        let resp: WorkflowResult = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.status, crate::workflows::WorkflowStatus::Success);
        assert_eq!(resp.steps.len(), 7);
    }

    #[actix_web::test]
    async fn voyage_launch_returns_status() {
        let test_app = test_state();
        let state = test_app.state.clone();
        let session = seed_session(&state.pool, &state.token_cipher);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let payload = LaunchRequest {
            voyage_id: "voyage-1".to_string(),
        };
        let req = test::TestRequest::post()
            .uri("/api/voyage/launch")
            .insert_header(auth_header(&session))
            .set_json(&payload)
            .to_request();
        let resp: LaunchResponse = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.voyage_id, "voyage-1");
        assert_eq!(resp.status, "launch queued");
    }

    #[actix_web::test]
    async fn openapi_json_returns_document() {
        let test_app = test_state();
        let app = test::init_service(
            App::new()
                .app_data(test_app.state.clone())
                .service(auth_config)
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
                .service(openapi_json),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/openapi.json")
            .to_request();
        let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;

        assert!(resp.get("openapi").is_some());
    }
}
