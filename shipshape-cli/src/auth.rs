//! Device flow authentication for the ShipShape CLI.
//!
//! Implements OAuth 2.0 Device Authorization Grant polling, token exchange,
//! and local session persistence for [`LoginArgs`].

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, Instant};

use clap::Args;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{CliError, CliResult};

/// Default base URL for the ShipShape server API.
const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8080";
/// OAuth 2.0 Device Authorization Grant type URI.
const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Default backoff penalty in seconds when GitHub returns `"slow_down"`.
const SLOW_DOWN_PENALTY_SECS: u64 = 5;

/// CLI arguments for the `login` subcommand.
#[derive(Args, Clone, Debug)]
pub struct LoginArgs {
    /// Base URL of the ShipShape server.
    #[arg(long, env = "SHIPSHAPE_API_URL", default_value = DEFAULT_SERVER_URL)]
    pub server_url: String,
    /// Override the auth session file path.
    #[arg(long)]
    pub auth_path: Option<PathBuf>,
}

/// Execute the device flow and store a ShipShape session token.
///
/// # Arguments
/// * `args` - Parsed CLI login arguments.
pub async fn run_login(args: LoginArgs) -> CliResult<()> {
    let client = ReqwestLoginClient::new()?;
    let sleeper = TokioSleeper;
    run_login_with(args, &client, &sleeper).await
}

/// Execute the device flow with injected [`LoginClient`] and [`Sleeper`] dependencies.
///
/// # Arguments
/// * `args` - Parsed CLI login arguments.
/// * `client` - HTTP client implementation for device flow endpoints.
/// * `sleeper` - Async sleep implementation for polling intervals.
pub(crate) async fn run_login_with<C: LoginClient, S: Sleeper>(
    args: LoginArgs,
    client: &C,
    sleeper: &S,
) -> CliResult<()> {
    let server_url = normalize_server_url(&args.server_url)?;
    let config = client.fetch_auth_config(&server_url).await?;
    let endpoints = derive_device_endpoints(&config.authorize_url)?;
    let device = client
        .request_device_code(
            &endpoints.device_code_url,
            &config.client_id,
            &config.scopes,
        )
        .await?;

    print_device_instructions(&device);

    let poller = LoginClientPoller::new(
        client,
        endpoints.token_url.clone(),
        config.client_id.clone(),
    );
    let github_token = poll_for_github_token(&poller, sleeper, &device).await?;
    let session = client
        .exchange_token_for_session(&server_url, &github_token)
        .await?;
    let auth_path = auth_store_path(args.auth_path)?;
    let stored = StoredAuthSession::new(&server_url, &session);
    write_auth_session(&auth_path, &stored)?;

    println!(
        "Authenticated as {}. Token stored at {}.",
        session.user.login,
        auth_path.display()
    );
    Ok(())
}

/// OAuth configuration payload returned by the server.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthConfigResponse {
    /// GitHub OAuth application client ID.
    pub(crate) client_id: String,
    /// GitHub OAuth authorization endpoint URL.
    pub(crate) authorize_url: String,
    /// Requested OAuth scopes.
    pub(crate) scopes: Vec<String>,
}

/// ShipShape session response returned after authentication.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthGithubResponse {
    /// Issued ShipShape bearer session token.
    pub(crate) token: String,
    /// Authenticated user profile.
    pub(crate) user: AuthUser,
}

/// Authenticated GitHub user details.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthUser {
    /// Internal ShipShape user UUID.
    pub(crate) id: String,
    /// GitHub username/login handle.
    pub(crate) login: String,
    /// GitHub numeric account identifier string.
    pub(crate) github_id: String,
}

/// Request payload for exchanging a GitHub access token.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthGithubTokenRequest {
    /// GitHub OAuth access token obtained from device flow.
    access_token: String,
}

/// Device flow initiation response from GitHub.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(crate) struct DeviceCodeResponse {
    /// Verification code for the device.
    pub(crate) device_code: String,
    /// User-visible code to enter at `verification_uri`.
    pub(crate) user_code: String,
    /// URL where the user enters `user_code`.
    pub(crate) verification_uri: String,
    /// Optional complete URL with `user_code` pre-populated.
    pub(crate) verification_uri_complete: Option<String>,
    /// Seconds until `device_code` expires.
    pub(crate) expires_in: u64,
    /// Minimum polling interval in seconds.
    pub(crate) interval: u64,
}

/// Device flow polling response from GitHub.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(crate) struct DeviceTokenResponse {
    /// Issued OAuth access token when authorization succeeds.
    pub(crate) access_token: Option<String>,
    /// OAuth error code (`"authorization_pending"`, `"slow_down"`, etc.).
    pub(crate) error: Option<String>,
    /// Human-readable description of the OAuth error.
    pub(crate) error_description: Option<String>,
    /// Updated polling interval in seconds when `"slow_down"` is returned.
    pub(crate) interval: Option<u64>,
}

/// Computed device flow endpoints derived from the OAuth authorize URL.
#[derive(Debug)]
struct DeviceEndpoints {
    /// GitHub device code request endpoint URL.
    device_code_url: String,
    /// GitHub OAuth access token polling endpoint URL.
    token_url: String,
}

/// Stored CLI auth session on disk.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredAuthSession {
    /// ShipShape server base URL associated with the session.
    server_url: String,
    /// ShipShape session bearer token.
    token: String,
    /// Authenticated user metadata.
    user: AuthUser,
}

impl StoredAuthSession {
    /// Build a new stored session record.
    ///
    /// # Arguments
    /// * `server_url` - Normalized ShipShape server URL.
    /// * `session` - Session response returned by the server.
    fn new(server_url: &str, session: &AuthGithubResponse) -> Self {
        Self {
            server_url: server_url.to_string(),
            token: session.token.clone(),
            user: session.user.clone(),
        }
    }
}

/// Normalize the server URL for consistent API requests.
///
/// # Arguments
/// * `server_url` - Raw server URL string.
fn normalize_server_url(server_url: &str) -> CliResult<String> {
    let trimmed = server_url.trim();
    if trimmed.is_empty() {
        return Err(CliError::EmptyServerUrl);
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

/// Derive device flow endpoints from the OAuth authorize URL.
///
/// # Arguments
/// * `authorize_url` - OAuth authorize endpoint URL ending in `/authorize`.
fn derive_device_endpoints(authorize_url: &str) -> CliResult<DeviceEndpoints> {
    let trimmed = authorize_url.trim_end_matches('/');
    let base = trimmed
        .strip_suffix("/authorize")
        .ok_or_else(|| CliError::InvalidAuthorizeUrl {
            url: authorize_url.to_string(),
        })?;
    let token_url = format!("{base}/access_token");
    let device_base = base.strip_suffix("/oauth").unwrap_or(base);
    let device_code_url = format!("{device_base}/device/code");
    Ok(DeviceEndpoints {
        device_code_url,
        token_url,
    })
}

/// Fetch OAuth configuration from the ShipShape server.
///
/// # Arguments
/// * `client` - Reqwest HTTP client.
/// * `server_url` - Normalized ShipShape server URL.
async fn fetch_auth_config(client: &Client, server_url: &str) -> CliResult<AuthConfigResponse> {
    let url = format!("{server_url}/auth/config");
    let response = client.get(url).send().await?.error_for_status()?;
    let config = response.json::<AuthConfigResponse>().await?;
    Ok(config)
}

/// Request the GitHub device code for the CLI login flow.
///
/// # Arguments
/// * `client` - Reqwest HTTP client.
/// * `device_code_url` - Device code endpoint URL.
/// * `client_id` - GitHub OAuth client ID.
/// * `scopes` - Requested OAuth scopes.
async fn request_device_code(
    client: &Client,
    device_code_url: &str,
    client_id: &str,
    scopes: &[String],
) -> CliResult<DeviceCodeResponse> {
    let scope = scopes.join(" ");
    let response = client
        .post(device_code_url)
        .header("Accept", "application/json")
        .form(&[("client_id", client_id), ("scope", scope.as_str())])
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json::<DeviceCodeResponse>().await?)
}

/// Poll the GitHub device token endpoint.
///
/// # Arguments
/// * `client` - Reqwest HTTP client.
/// * `token_url` - Access token endpoint URL.
/// * `client_id` - GitHub OAuth client ID.
/// * `device_code` - Device code being polled.
async fn poll_device_token(
    client: &Client,
    token_url: &str,
    client_id: &str,
    device_code: &str,
) -> CliResult<DeviceTokenResponse> {
    let response = client
        .post(token_url)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", DEVICE_GRANT_TYPE),
        ])
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json::<DeviceTokenResponse>().await?)
}

/// Exchange a GitHub access token for a ShipShape session token.
///
/// # Arguments
/// * `client` - Reqwest HTTP client.
/// * `server_url` - Normalized ShipShape server URL.
/// * `github_token` - GitHub OAuth access token.
async fn exchange_token_for_session(
    client: &Client,
    server_url: &str,
    github_token: &str,
) -> CliResult<AuthGithubResponse> {
    let payload = AuthGithubTokenRequest {
        access_token: github_token.to_string(),
    };
    let response = client
        .post(format!("{server_url}/auth/github/token"))
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json::<AuthGithubResponse>().await?)
}

/// Print instructions for completing the device flow.
///
/// # Arguments
/// * `device` - Device code response containing user code and verification URLs.
fn print_device_instructions(device: &DeviceCodeResponse) {
    println!(
        "Open {} and enter code {} to authorize ShipShape.",
        device.verification_uri, device.user_code
    );
    if let Some(url) = device.verification_uri_complete.as_ref() {
        println!("Or open {url} to authorize automatically.");
    }
    println!(
        "Waiting for authorization (expires in {} seconds)...",
        device.expires_in
    );
}

/// Parsed state from a device flow poll response.
#[derive(Debug, PartialEq, Eq)]
enum DevicePollOutcome {
    /// User completed authorization and a token was issued.
    Success(String),
    /// Authorization is still pending user action.
    Pending,
    /// Polling too fast; slow down to the optional interval.
    SlowDown(Option<u64>),
    /// The device code expired before user authorization.
    Expired,
    /// The user denied the authorization request.
    Denied,
    /// An unexpected OAuth error occurred.
    Error(String),
}

/// Interpret a device flow token response into an actionable [`DevicePollOutcome`].
///
/// # Arguments
/// * `response` - Parsed JSON response from the device token endpoint.
fn interpret_device_token(response: DeviceTokenResponse) -> DevicePollOutcome {
    if let Some(token) = response.access_token {
        return DevicePollOutcome::Success(token);
    }
    let Some(error) = response.error else {
        return DevicePollOutcome::Error("missing device flow token".to_string());
    };
    match error.as_str() {
        "authorization_pending" => DevicePollOutcome::Pending,
        "slow_down" => DevicePollOutcome::SlowDown(response.interval),
        "expired_token" => DevicePollOutcome::Expired,
        "access_denied" => DevicePollOutcome::Denied,
        _ => DevicePollOutcome::Error(
            response
                .error_description
                .unwrap_or_else(|| format!("device flow error: {error}")),
        ),
    }
}

/// HTTP client abstraction for the login flow.
pub(crate) trait LoginClient {
    /// Fetch OAuth configuration from `server_url`.
    ///
    /// # Arguments
    /// * `server_url` - Normalized ShipShape server URL.
    fn fetch_auth_config<'a>(
        &'a self,
        server_url: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<AuthConfigResponse>> + Send + 'a>>;

    /// Request a GitHub device code from `device_code_url`.
    ///
    /// # Arguments
    /// * `device_code_url` - Device code endpoint URL.
    /// * `client_id` - OAuth client ID.
    /// * `scopes` - Requested OAuth scopes.
    fn request_device_code<'a>(
        &'a self,
        device_code_url: &'a str,
        client_id: &'a str,
        scopes: &'a [String],
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceCodeResponse>> + Send + 'a>>;

    /// Poll `token_url` for a device flow token.
    ///
    /// # Arguments
    /// * `token_url` - Token endpoint URL.
    /// * `client_id` - OAuth client ID.
    /// * `device_code` - Device code to poll.
    fn poll_device_token<'a>(
        &'a self,
        token_url: &'a str,
        client_id: &'a str,
        device_code: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>>;

    /// Exchange `github_token` for a ShipShape session at `server_url`.
    ///
    /// # Arguments
    /// * `server_url` - Normalized ShipShape server URL.
    /// * `github_token` - GitHub access token.
    fn exchange_token_for_session<'a>(
        &'a self,
        server_url: &'a str,
        github_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<AuthGithubResponse>> + Send + 'a>>;
}

/// Reqwest-backed login client.
struct ReqwestLoginClient {
    /// Underlying async Reqwest HTTP client.
    client: Client,
}

impl ReqwestLoginClient {
    /// Build a new reqwest login client.
    fn new() -> CliResult<Self> {
        let client = Client::builder().user_agent("shipshape-cli").build()?;
        Ok(Self { client })
    }
}

impl LoginClient for ReqwestLoginClient {
    fn fetch_auth_config<'a>(
        &'a self,
        server_url: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<AuthConfigResponse>> + Send + 'a>> {
        Box::pin(fetch_auth_config(&self.client, server_url))
    }

    fn request_device_code<'a>(
        &'a self,
        device_code_url: &'a str,
        client_id: &'a str,
        scopes: &'a [String],
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceCodeResponse>> + Send + 'a>> {
        Box::pin(request_device_code(
            &self.client,
            device_code_url,
            client_id,
            scopes,
        ))
    }

    fn poll_device_token<'a>(
        &'a self,
        token_url: &'a str,
        client_id: &'a str,
        device_code: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>> {
        Box::pin(poll_device_token(
            &self.client,
            token_url,
            client_id,
            device_code,
        ))
    }

    fn exchange_token_for_session<'a>(
        &'a self,
        server_url: &'a str,
        github_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<AuthGithubResponse>> + Send + 'a>> {
        Box::pin(exchange_token_for_session(
            &self.client,
            server_url,
            github_token,
        ))
    }
}

/// Polls the GitHub device flow token endpoint.
trait DeviceTokenPoller {
    /// Poll once for `device_code`.
    ///
    /// # Arguments
    /// * `device_code` - Device code to query.
    fn poll<'a>(
        &'a self,
        device_code: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>>;
}

/// Device flow poller that delegates to a [`LoginClient`].
struct LoginClientPoller<'a, C: LoginClient> {
    /// Reference to the login client.
    client: &'a C,
    /// GitHub access token endpoint URL.
    token_url: String,
    /// GitHub OAuth client ID.
    client_id: String,
}

impl<'a, C: LoginClient> LoginClientPoller<'a, C> {
    /// Create a new poller for a login client.
    ///
    /// # Arguments
    /// * `client` - Login client reference.
    /// * `token_url` - GitHub access token endpoint URL.
    /// * `client_id` - GitHub OAuth client ID.
    fn new(client: &'a C, token_url: String, client_id: String) -> Self {
        Self {
            client,
            token_url,
            client_id,
        }
    }
}

impl<C: LoginClient> DeviceTokenPoller for LoginClientPoller<'_, C> {
    fn poll<'a>(
        &'a self,
        device_code: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>> {
        self.client
            .poll_device_token(&self.token_url, &self.client_id, device_code)
    }
}

/// Async sleep abstraction for polling tests.
pub(crate) trait Sleeper {
    /// Sleep asynchronously for `duration`.
    ///
    /// # Arguments
    /// * `duration` - Wait duration before the next poll.
    fn sleep<'a>(&'a self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

/// Tokio-backed sleeper used in production.
struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep<'a>(&'a self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// Poll GitHub until a device token is available or expired.
///
/// # Arguments
/// * `poller` - Device token poller implementation.
/// * `sleeper` - Async sleeper implementation.
/// * `device` - Device code initiation response.
async fn poll_for_github_token<P: DeviceTokenPoller, S: Sleeper>(
    poller: &P,
    sleeper: &S,
    device: &DeviceCodeResponse,
) -> CliResult<String> {
    let mut interval = Duration::from_secs(device.interval.max(1));
    let deadline = Instant::now() + Duration::from_secs(device.expires_in);
    loop {
        if Instant::now() >= deadline {
            return Err(CliError::DeviceAuthorizationTimedOut);
        }
        let response = poller.poll(&device.device_code).await?;
        match interpret_device_token(response) {
            DevicePollOutcome::Success(token) => return Ok(token),
            DevicePollOutcome::Pending => {
                sleeper.sleep(interval).await;
            }
            DevicePollOutcome::SlowDown(new_interval) => {
                interval = match new_interval {
                    Some(secs) => Duration::from_secs(secs),
                    None => interval + Duration::from_secs(SLOW_DOWN_PENALTY_SECS),
                };
                sleeper.sleep(interval).await;
            }
            DevicePollOutcome::Expired => return Err(CliError::DeviceCodeExpired),
            DevicePollOutcome::Denied => return Err(CliError::DeviceAccessDenied),
            DevicePollOutcome::Error(message) => {
                return Err(CliError::DeviceFlow { message });
            }
        }
    }
}

/// Resolve the local path where auth sessions are stored.
///
/// # Arguments
/// * `auth_path` - Optional explicit file path override.
fn auth_store_path(auth_path: Option<PathBuf>) -> CliResult<PathBuf> {
    if let Some(path) = auth_path {
        return Ok(path);
    }
    if let Ok(path) = std::env::var("SHIPSHAPE_AUTH_PATH")
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }
    if let Ok(base) = std::env::var("XDG_CONFIG_HOME")
        && !base.trim().is_empty()
    {
        return Ok(PathBuf::from(base).join("shipshape").join("auth.json"));
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return Ok(PathBuf::from(home).join(".config/shipshape/auth.json"));
    }
    Err(CliError::AuthStoragePathUnavailable)
}

/// Persist the auth session JSON to disk.
///
/// # Arguments
/// * `path` - Destination file path.
/// * `session` - Session record to serialize.
fn write_auth_session(path: &Path, session: &StoredAuthSession) -> CliResult<()> {
    let payload = serde_json::to_string_pretty(session)?;
    shipshape_core::FileSystem::write_file(&shipshape_core::StdFileSystem::new(), path, &payload)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::{Mutex, OnceLock};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;

    /// Global mutex for serialized environment-variable tests.
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    /// Acquire the global environment-variable mutex for serialized env tests.
    fn env_lock() -> CliResult<std::sync::MutexGuard<'static, ()>> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| CliError::AuthStoragePathUnavailable)
    }

    /// RAII guard for setting and restoring an environment variable in tests.
    struct EnvGuard {
        /// Environment variable name.
        key: &'static str,
        /// Previous value prior to override.
        prev: Option<String>,
    }

    impl EnvGuard {
        /// Set or remove environment variable `key`.
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let prev = std::env::var(key).ok();
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(prev) = self.prev.take() {
                unsafe { std::env::set_var(self.key, prev) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    /// Poller returning a fixed sequence of [`DeviceTokenResponse`] items.
    struct SequencePoller {
        /// Queued responses.
        responses: RefCell<VecDeque<DeviceTokenResponse>>,
    }

    impl SequencePoller {
        /// Create a new sequence poller.
        fn new(responses: Vec<DeviceTokenResponse>) -> Self {
            Self {
                responses: RefCell::new(responses.into()),
            }
        }
    }

    impl DeviceTokenPoller for SequencePoller {
        fn poll<'a>(
            &'a self,
            _device_code: &'a str,
        ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>> {
            let res = self
                .responses
                .borrow_mut()
                .pop_front()
                .ok_or(CliError::DeviceAuthorizationTimedOut);
            Box::pin(async move { res })
        }
    }

    /// Sleeper that records requested sleep durations in memory.
    struct RecordingSleeper {
        /// Recorded sleep durations.
        durations: RefCell<Vec<Duration>>,
    }

    impl RecordingSleeper {
        /// Create an empty recording sleeper.
        fn new() -> Self {
            Self {
                durations: RefCell::new(Vec::new()),
            }
        }

        /// Return a snapshot of recorded durations.
        fn durations(&self) -> Vec<Duration> {
            self.durations.borrow().clone()
        }
    }

    impl Sleeper for RecordingSleeper {
        fn sleep<'a>(
            &'a self,
            duration: Duration,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            self.durations.borrow_mut().push(duration);
            Box::pin(async move {})
        }
    }

    #[test]
    fn normalize_server_url_trims_trailing_slash_and_rejects_empty() -> CliResult<()> {
        let url = normalize_server_url("http://localhost:8080/")?;
        assert_eq!(url, "http://localhost:8080");

        let err = normalize_server_url("   ");
        assert!(matches!(err, Err(CliError::EmptyServerUrl)));
        Ok(())
    }

    #[test]
    fn derive_device_endpoints_handles_standard_and_invalid_urls() -> CliResult<()> {
        let endpoints = derive_device_endpoints("https://github.com/login/oauth/authorize")?;
        assert_eq!(
            endpoints.device_code_url,
            "https://github.com/login/device/code"
        );
        assert_eq!(
            endpoints.token_url,
            "https://github.com/login/oauth/access_token"
        );

        let err = derive_device_endpoints("https://github.com/login/oauth");
        assert!(matches!(err, Err(CliError::InvalidAuthorizeUrl { .. })));
        Ok(())
    }

    #[test]
    fn interpret_device_token_covers_all_outcomes() {
        assert_eq!(
            interpret_device_token(DeviceTokenResponse {
                access_token: Some("token".to_string()),
                error: None,
                error_description: None,
                interval: None,
            }),
            DevicePollOutcome::Success("token".to_string())
        );

        assert_eq!(
            interpret_device_token(DeviceTokenResponse {
                access_token: None,
                error: Some("authorization_pending".to_string()),
                error_description: None,
                interval: None,
            }),
            DevicePollOutcome::Pending
        );

        assert_eq!(
            interpret_device_token(DeviceTokenResponse {
                access_token: None,
                error: Some("slow_down".to_string()),
                error_description: None,
                interval: Some(9),
            }),
            DevicePollOutcome::SlowDown(Some(9))
        );

        assert_eq!(
            interpret_device_token(DeviceTokenResponse {
                access_token: None,
                error: Some("expired_token".to_string()),
                error_description: None,
                interval: None,
            }),
            DevicePollOutcome::Expired
        );

        assert_eq!(
            interpret_device_token(DeviceTokenResponse {
                access_token: None,
                error: Some("access_denied".to_string()),
                error_description: None,
                interval: None,
            }),
            DevicePollOutcome::Denied
        );

        assert_eq!(
            interpret_device_token(DeviceTokenResponse {
                access_token: None,
                error: Some("invalid_scope".to_string()),
                error_description: Some("scope bad".to_string()),
                interval: None,
            }),
            DevicePollOutcome::Error("scope bad".to_string())
        );

        assert_eq!(
            interpret_device_token(DeviceTokenResponse {
                access_token: None,
                error: Some("custom_err".to_string()),
                error_description: None,
                interval: None,
            }),
            DevicePollOutcome::Error("device flow error: custom_err".to_string())
        );

        assert_eq!(
            interpret_device_token(DeviceTokenResponse {
                access_token: None,
                error: None,
                error_description: None,
                interval: None,
            }),
            DevicePollOutcome::Error("missing device flow token".to_string())
        );
    }

    #[tokio::test]
    async fn poll_for_github_token_handles_pending_slowdown_and_errors() -> CliResult<()> {
        let poller = SequencePoller::new(vec![
            DeviceTokenResponse {
                access_token: None,
                error: Some("authorization_pending".to_string()),
                error_description: None,
                interval: None,
            },
            DeviceTokenResponse {
                access_token: None,
                error: Some("slow_down".to_string()),
                error_description: None,
                interval: Some(7),
            },
            DeviceTokenResponse {
                access_token: None,
                error: Some("slow_down".to_string()),
                error_description: None,
                interval: None,
            },
            DeviceTokenResponse {
                access_token: Some("token-123".to_string()),
                error: None,
                error_description: None,
                interval: None,
            },
        ]);
        let sleeper = RecordingSleeper::new();
        let device = DeviceCodeResponse {
            device_code: "dev".to_string(),
            user_code: "code".to_string(),
            verification_uri: "https://verify".to_string(),
            verification_uri_complete: None,
            expires_in: 30,
            interval: 2,
        };

        let token = poll_for_github_token(&poller, &sleeper, &device).await?;
        assert_eq!(token, "token-123");
        assert_eq!(
            sleeper.durations(),
            vec![
                Duration::from_secs(2),
                Duration::from_secs(7),
                Duration::from_secs(12),
            ]
        );

        // Expired
        let expired_poller = SequencePoller::new(vec![DeviceTokenResponse {
            access_token: None,
            error: Some("expired_token".to_string()),
            error_description: None,
            interval: None,
        }]);
        assert!(matches!(
            poll_for_github_token(&expired_poller, &sleeper, &device).await,
            Err(CliError::DeviceCodeExpired)
        ));

        // Denied
        let denied_poller = SequencePoller::new(vec![DeviceTokenResponse {
            access_token: None,
            error: Some("access_denied".to_string()),
            error_description: None,
            interval: None,
        }]);
        assert!(matches!(
            poll_for_github_token(&denied_poller, &sleeper, &device).await,
            Err(CliError::DeviceAccessDenied)
        ));

        // Custom error
        let custom_poller = SequencePoller::new(vec![DeviceTokenResponse {
            access_token: None,
            error: Some("bad_device".to_string()),
            error_description: Some("bad code".to_string()),
            interval: None,
        }]);
        assert!(matches!(
            poll_for_github_token(&custom_poller, &sleeper, &device).await,
            Err(CliError::DeviceFlow { .. })
        ));

        // Timeout
        let timeout_device = DeviceCodeResponse {
            expires_in: 0,
            ..device
        };
        assert!(matches!(
            poll_for_github_token(&custom_poller, &sleeper, &timeout_device).await,
            Err(CliError::DeviceAuthorizationTimedOut)
        ));
        Ok(())
    }

    #[test]
    fn auth_store_path_resolves_explicit_and_env_fallbacks() -> CliResult<()> {
        let explicit = auth_store_path(Some(PathBuf::from("/tmp/shipshape-auth.json")))?;
        assert_eq!(explicit, PathBuf::from("/tmp/shipshape-auth.json"));

        let _lock = env_lock()?;
        {
            let _g = EnvGuard::set("SHIPSHAPE_AUTH_PATH", Some("/tmp/shipshape-env.json"));
            assert_eq!(
                auth_store_path(None)?,
                PathBuf::from("/tmp/shipshape-env.json")
            );
        }
        {
            let _g1 = EnvGuard::set("SHIPSHAPE_AUTH_PATH", None);
            let _g2 = EnvGuard::set("XDG_CONFIG_HOME", Some("/tmp/xdg"));
            assert_eq!(
                auth_store_path(None)?,
                PathBuf::from("/tmp/xdg/shipshape/auth.json")
            );
        }
        {
            let _g1 = EnvGuard::set("SHIPSHAPE_AUTH_PATH", None);
            let _g2 = EnvGuard::set("XDG_CONFIG_HOME", None);
            let _g3 = EnvGuard::set("HOME", Some("/tmp/home"));
            assert_eq!(
                auth_store_path(None)?,
                PathBuf::from("/tmp/home/.config/shipshape/auth.json")
            );
        }
        {
            let _g1 = EnvGuard::set("SHIPSHAPE_AUTH_PATH", None);
            let _g2 = EnvGuard::set("XDG_CONFIG_HOME", None);
            let _g3 = EnvGuard::set("HOME", None);
            assert!(matches!(
                auth_store_path(None),
                Err(CliError::AuthStoragePathUnavailable)
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn run_login_end_to_end_over_mock_http_and_tokio_sleeper() -> CliResult<()> {
        TokioSleeper.sleep(Duration::from_millis(1)).await;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let base_url = format!("http://{addr}");
        let authorize_url = format!("{base_url}/login/oauth/authorize");
        let verify_url = format!("{base_url}/login/device");
        let verify_complete = format!("{base_url}/login/device?code=ABCD-1234");

        let server = tokio::spawn(async move {
            for step in 0..4 {
                let (mut stream, _) = listener.accept().await?;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let body = match step {
                    0 => format!(
                        r#"{{"clientId":"cid","authorizeUrl":"{authorize_url}","scopes":["read:user","repo"]}}"#
                    ),
                    1 => format!(
                        r#"{{"device_code":"dev-1","user_code":"ABCD-1234","verification_uri":"{verify_url}","verification_uri_complete":"{verify_complete}","expires_in":600,"interval":1}}"#
                    ),
                    2 => r#"{"access_token":"gh-tok-1"}"#.to_string(),
                    _ => {
                        r#"{"token":"ss-tok-1","user":{"id":"u1","login":"pilot","githubId":"42"}}"#
                            .to_string()
                    }
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
            Ok::<(), std::io::Error>(())
        });

        let root = std::env::temp_dir().join("shipshape_cli_mock_http_login");
        let auth_path = root.join("nested").join("auth.json");
        let args = LoginArgs {
            server_url: base_url,
            auth_path: Some(auth_path.clone()),
        };

        run_login(args).await?;
        let _ = server.await;

        let contents = tokio::fs::read_to_string(&auth_path).await?;
        let parsed: StoredAuthSession = serde_json::from_str(&contents)?;
        assert_eq!(parsed.token, "ss-tok-1");
        assert_eq!(parsed.user.login, "pilot");

        let _ = tokio::fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[test]
    fn print_instructions_handles_missing_complete_uri() {
        let device = DeviceCodeResponse {
            device_code: "d".to_string(),
            user_code: "U-1".to_string(),
            verification_uri: "https://example.com/device".to_string(),
            verification_uri_complete: None,
            expires_in: 60,
            interval: 5,
        };
        print_device_instructions(&device);
    }
}
