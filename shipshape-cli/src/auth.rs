//! Device flow authentication for the ShipShape CLI.

use crate::CliResult;
use clap::Args;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, Instant};

const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8080";
#[cfg_attr(test, allow(dead_code))]
const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const SLOW_DOWN_PENALTY_SECS: u64 = 5;

/// CLI arguments for the login command.
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
#[cfg(not(test))]
pub async fn run_login(args: LoginArgs) -> CliResult<()> {
    let client = ReqwestLoginClient::new()?;
    let sleeper = TokioSleeper;
    run_login_with(args, &client, &sleeper).await
}

/// Execute the device flow with injected dependencies.
async fn run_login_with<C: LoginClient, S: Sleeper>(
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
    write_auth_session(&auth_path, &stored).await?;

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
struct AuthConfigResponse {
    client_id: String,
    authorize_url: String,
    scopes: Vec<String>,
}

/// ShipShape session response returned after authentication.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AuthGithubResponse {
    token: String,
    user: AuthUser,
}

/// Authenticated GitHub user details.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AuthUser {
    id: String,
    login: String,
    github_id: String,
}

/// Request payload for exchanging a GitHub access token.
#[cfg_attr(test, allow(dead_code))]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthGithubTokenRequest {
    access_token: String,
}

/// Device flow initiation response from GitHub.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: u64,
}

/// Device flow polling response from GitHub.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
struct DeviceTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
    interval: Option<u64>,
}

/// Computed device flow endpoints derived from the OAuth authorize URL.
#[derive(Debug)]
struct DeviceEndpoints {
    device_code_url: String,
    token_url: String,
}

/// Stored CLI auth session on disk.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredAuthSession {
    server_url: String,
    token: String,
    user: AuthUser,
}

impl StoredAuthSession {
    /// Build a new stored session record.
    fn new(server_url: &str, session: &AuthGithubResponse) -> Self {
        Self {
            server_url: server_url.to_string(),
            token: session.token.clone(),
            user: session.user.clone(),
        }
    }
}

/// Normalize the server URL for consistent API requests.
fn normalize_server_url(server_url: &str) -> CliResult<String> {
    let trimmed = server_url.trim();
    if trimmed.is_empty() {
        return Err("server url is required".into());
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

/// Derive device flow endpoints from the OAuth authorize URL.
fn derive_device_endpoints(authorize_url: &str) -> CliResult<DeviceEndpoints> {
    let trimmed = authorize_url.trim_end_matches('/');
    let base = trimmed
        .strip_suffix("/authorize")
        .ok_or_else(|| "authorize_url missing /authorize suffix".to_string())?;
    let token_url = format!("{base}/access_token");
    let device_base = base.strip_suffix("/oauth").unwrap_or(base);
    let device_code_url = format!("{device_base}/device/code");
    Ok(DeviceEndpoints {
        device_code_url,
        token_url,
    })
}

/// Fetch OAuth configuration from the ShipShape server.
#[cfg_attr(test, allow(dead_code))]
async fn fetch_auth_config(client: &Client, server_url: &str) -> CliResult<AuthConfigResponse> {
    let url = format!("{server_url}/auth/config");
    let response = client.get(url).send().await?.error_for_status()?;
    let config = response.json::<AuthConfigResponse>().await?;
    Ok(config)
}

/// Request the GitHub device code for the CLI login flow.
#[cfg_attr(test, allow(dead_code))]
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
#[cfg_attr(test, allow(dead_code))]
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
#[cfg_attr(test, allow(dead_code))]
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
fn print_device_instructions(device: &DeviceCodeResponse) {
    println!(
        "Open {} and enter code {} to authorize ShipShape.",
        device.verification_uri, device.user_code
    );
    if let Some(url) = device.verification_uri_complete.as_ref() {
        println!("Or open {} to authorize automatically.", url);
    }
    println!(
        "Waiting for authorization (expires in {} seconds)...",
        device.expires_in
    );
}

/// Parsed state from a device flow poll response.
#[derive(Debug, PartialEq, Eq)]
enum DevicePollOutcome {
    Success(String),
    Pending,
    SlowDown(Option<u64>),
    Expired(String),
    Denied(String),
    Error(String),
}

/// Interpret a device flow token response into an actionable outcome.
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
        "expired_token" => DevicePollOutcome::Expired("device code expired".to_string()),
        "access_denied" => DevicePollOutcome::Denied("access denied".to_string()),
        _ => DevicePollOutcome::Error(
            response
                .error_description
                .unwrap_or_else(|| format!("device flow error: {error}")),
        ),
    }
}

/// HTTP client abstraction for the login flow.
trait LoginClient {
    fn fetch_auth_config<'a>(
        &'a self,
        server_url: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<AuthConfigResponse>> + Send + 'a>>;

    fn request_device_code<'a>(
        &'a self,
        device_code_url: &'a str,
        client_id: &'a str,
        scopes: &'a [String],
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceCodeResponse>> + Send + 'a>>;

    fn poll_device_token<'a>(
        &'a self,
        token_url: &'a str,
        client_id: &'a str,
        device_code: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>>;

    fn exchange_token_for_session<'a>(
        &'a self,
        server_url: &'a str,
        github_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<AuthGithubResponse>> + Send + 'a>>;
}

/// Reqwest-backed login client.
#[cfg_attr(test, allow(dead_code))]
struct ReqwestLoginClient {
    client: Client,
}

impl ReqwestLoginClient {
    /// Build a new reqwest login client.
    #[cfg_attr(test, allow(dead_code))]
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
    fn poll<'a>(
        &'a self,
        device_code: &'a str,
    ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>>;
}

/// Device flow poller that delegates to a login client.
struct LoginClientPoller<'a, C: LoginClient> {
    client: &'a C,
    token_url: String,
    client_id: String,
}

impl<'a, C: LoginClient> LoginClientPoller<'a, C> {
    /// Create a new poller for a login client.
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
trait Sleeper {
    fn sleep<'a>(&'a self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

/// Tokio-backed sleeper used in production.
#[cfg_attr(test, allow(dead_code))]
struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep<'a>(&'a self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// Poll GitHub until a device token is available or expired.
async fn poll_for_github_token<P: DeviceTokenPoller, S: Sleeper>(
    poller: &P,
    sleeper: &S,
    device: &DeviceCodeResponse,
) -> CliResult<String> {
    let mut interval = Duration::from_secs(device.interval.max(1));
    let deadline = Instant::now() + Duration::from_secs(device.expires_in);
    loop {
        if Instant::now() >= deadline {
            return Err("device authorization timed out".into());
        }
        let response = poller.poll(&device.device_code).await?;
        match interpret_device_token(response) {
            DevicePollOutcome::Success(token) => return Ok(token),
            DevicePollOutcome::Pending => {
                sleeper.sleep(interval).await;
            }
            DevicePollOutcome::SlowDown(new_interval) => {
                interval = new_interval
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| interval + Duration::from_secs(SLOW_DOWN_PENALTY_SECS));
                sleeper.sleep(interval).await;
            }
            DevicePollOutcome::Expired(message)
            | DevicePollOutcome::Denied(message)
            | DevicePollOutcome::Error(message) => return Err(message.into()),
        }
    }
}

/// Resolve the local path where auth sessions are stored.
fn auth_store_path(auth_path: Option<PathBuf>) -> CliResult<PathBuf> {
    if let Some(path) = auth_path {
        return Ok(path);
    }
    if let Ok(path) = std::env::var("SHIPSHAPE_AUTH_PATH") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    if let Ok(base) = std::env::var("XDG_CONFIG_HOME") {
        if !base.trim().is_empty() {
            return Ok(PathBuf::from(base).join("shipshape").join("auth.json"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Ok(PathBuf::from(home).join(".config/shipshape/auth.json"));
        }
    }
    Err("unable to resolve auth storage path".into())
}

/// Persist the auth session JSON to disk.
async fn write_auth_session(path: &Path, session: &StoredAuthSession) -> CliResult<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let payload = serde_json::to_vec_pretty(session)?;
    tokio::fs::write(path, payload).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
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

    struct SequencePoller {
        responses: Mutex<VecDeque<DeviceTokenResponse>>,
    }

    impl SequencePoller {
        fn new(responses: Vec<DeviceTokenResponse>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
            }
        }

        async fn poll_next(&self) -> CliResult<DeviceTokenResponse> {
            let mut guard = self.responses.lock().expect("poller lock");
            Ok(guard.pop_front().expect("no more device token responses"))
        }
    }

    impl DeviceTokenPoller for SequencePoller {
        fn poll<'a>(
            &'a self,
            _device_code: &'a str,
        ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>> {
            Box::pin(self.poll_next())
        }
    }

    struct RecordingSleeper {
        durations: Mutex<Vec<Duration>>,
    }

    impl RecordingSleeper {
        fn new() -> Self {
            Self {
                durations: Mutex::new(Vec::new()),
            }
        }

        fn durations(&self) -> Vec<Duration> {
            self.durations.lock().expect("durations").clone()
        }
    }

    impl Sleeper for RecordingSleeper {
        fn sleep<'a>(
            &'a self,
            duration: Duration,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async move {
                self.durations.lock().expect("durations").push(duration);
            })
        }
    }

    struct TestLoginClient {
        config: AuthConfigResponse,
        device: DeviceCodeResponse,
        tokens: Mutex<VecDeque<DeviceTokenResponse>>,
        session: AuthGithubResponse,
    }

    impl TestLoginClient {
        fn new(
            config: AuthConfigResponse,
            device: DeviceCodeResponse,
            tokens: Vec<DeviceTokenResponse>,
            session: AuthGithubResponse,
        ) -> Self {
            Self {
                config,
                device,
                tokens: Mutex::new(tokens.into()),
                session,
            }
        }
    }

    impl LoginClient for TestLoginClient {
        fn fetch_auth_config<'a>(
            &'a self,
            _server_url: &'a str,
        ) -> Pin<Box<dyn Future<Output = CliResult<AuthConfigResponse>> + Send + 'a>> {
            let config = self.config.clone();
            Box::pin(async move { Ok(config) })
        }

        fn request_device_code<'a>(
            &'a self,
            _device_code_url: &'a str,
            _client_id: &'a str,
            _scopes: &'a [String],
        ) -> Pin<Box<dyn Future<Output = CliResult<DeviceCodeResponse>> + Send + 'a>> {
            let device = self.device.clone();
            Box::pin(async move { Ok(device) })
        }

        fn poll_device_token<'a>(
            &'a self,
            _token_url: &'a str,
            _client_id: &'a str,
            _device_code: &'a str,
        ) -> Pin<Box<dyn Future<Output = CliResult<DeviceTokenResponse>> + Send + 'a>> {
            Box::pin(async move {
                let mut guard = self.tokens.lock().expect("tokens lock");
                Ok(guard.pop_front().expect("no more device token responses"))
            })
        }

        fn exchange_token_for_session<'a>(
            &'a self,
            _server_url: &'a str,
            _github_token: &'a str,
        ) -> Pin<Box<dyn Future<Output = CliResult<AuthGithubResponse>> + Send + 'a>> {
            let session = self.session.clone();
            Box::pin(async move { Ok(session) })
        }
    }

    #[test]
    fn normalize_server_url_trims_trailing_slash() {
        let url = normalize_server_url("http://localhost:8080/").expect("url");
        assert_eq!(url, "http://localhost:8080");
    }

    #[test]
    fn normalize_server_url_rejects_empty() {
        let err = normalize_server_url("   ").unwrap_err();
        assert!(err.to_string().contains("server url"));
    }

    #[test]
    fn derive_device_endpoints_handles_standard_urls() {
        let endpoints =
            derive_device_endpoints("https://github.com/login/oauth/authorize").expect("endpoints");
        assert_eq!(
            endpoints.device_code_url,
            "https://github.com/login/device/code"
        );
        assert_eq!(
            endpoints.token_url,
            "https://github.com/login/oauth/access_token"
        );
    }

    #[test]
    fn derive_device_endpoints_errors_without_authorize_suffix() {
        let err = derive_device_endpoints("https://github.com/login/oauth")
            .unwrap_err()
            .to_string();
        assert!(err.contains("authorize"));
    }

    #[test]
    fn interpret_device_token_handles_success() {
        let outcome = interpret_device_token(DeviceTokenResponse {
            access_token: Some("token".to_string()),
            error: None,
            error_description: None,
            interval: None,
        });
        assert_eq!(outcome, DevicePollOutcome::Success("token".to_string()));
    }

    #[test]
    fn interpret_device_token_handles_pending() {
        let outcome = interpret_device_token(DeviceTokenResponse {
            access_token: None,
            error: Some("authorization_pending".to_string()),
            error_description: None,
            interval: None,
        });
        assert_eq!(outcome, DevicePollOutcome::Pending);
    }

    #[test]
    fn interpret_device_token_handles_slow_down() {
        let outcome = interpret_device_token(DeviceTokenResponse {
            access_token: None,
            error: Some("slow_down".to_string()),
            error_description: None,
            interval: Some(9),
        });
        assert_eq!(outcome, DevicePollOutcome::SlowDown(Some(9)));
    }

    #[test]
    fn interpret_device_token_handles_expired() {
        let outcome = interpret_device_token(DeviceTokenResponse {
            access_token: None,
            error: Some("expired_token".to_string()),
            error_description: None,
            interval: None,
        });
        assert_eq!(
            outcome,
            DevicePollOutcome::Expired("device code expired".to_string())
        );
    }

    #[test]
    fn interpret_device_token_handles_denied() {
        let outcome = interpret_device_token(DeviceTokenResponse {
            access_token: None,
            error: Some("access_denied".to_string()),
            error_description: None,
            interval: None,
        });
        assert_eq!(
            outcome,
            DevicePollOutcome::Denied("access denied".to_string())
        );
    }

    #[test]
    fn interpret_device_token_handles_unknown_error() {
        let outcome = interpret_device_token(DeviceTokenResponse {
            access_token: None,
            error: Some("invalid_scope".to_string()),
            error_description: Some("scope bad".to_string()),
            interval: None,
        });
        assert_eq!(outcome, DevicePollOutcome::Error("scope bad".to_string()));
    }

    #[test]
    fn interpret_device_token_requires_token_or_error() {
        let outcome = interpret_device_token(DeviceTokenResponse {
            access_token: None,
            error: None,
            error_description: None,
            interval: None,
        });
        assert_eq!(
            outcome,
            DevicePollOutcome::Error("missing device flow token".to_string())
        );
    }

    #[tokio::test]
    async fn poll_for_github_token_returns_success() {
        let poller = SequencePoller::new(vec![
            DeviceTokenResponse {
                access_token: None,
                error: Some("authorization_pending".to_string()),
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

        let token = poll_for_github_token(&poller, &sleeper, &device)
            .await
            .expect("token");
        assert_eq!(token, "token-123");
        assert_eq!(sleeper.durations(), vec![Duration::from_secs(2)]);
    }

    #[tokio::test]
    async fn poll_for_github_token_slow_down_updates_interval() {
        let poller = SequencePoller::new(vec![
            DeviceTokenResponse {
                access_token: None,
                error: Some("slow_down".to_string()),
                error_description: None,
                interval: Some(7),
            },
            DeviceTokenResponse {
                access_token: Some("token-456".to_string()),
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
            interval: 1,
        };

        let token = poll_for_github_token(&poller, &sleeper, &device)
            .await
            .expect("token");
        assert_eq!(token, "token-456");
        assert_eq!(sleeper.durations(), vec![Duration::from_secs(7)]);
    }

    #[tokio::test]
    async fn poll_for_github_token_slow_down_defaults_penalty() {
        let poller = SequencePoller::new(vec![
            DeviceTokenResponse {
                access_token: None,
                error: Some("slow_down".to_string()),
                error_description: None,
                interval: None,
            },
            DeviceTokenResponse {
                access_token: Some("token-789".to_string()),
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
            interval: 3,
        };

        let token = poll_for_github_token(&poller, &sleeper, &device)
            .await
            .expect("token");
        assert_eq!(token, "token-789");
        assert_eq!(sleeper.durations(), vec![Duration::from_secs(8)]);
    }

    #[tokio::test]
    async fn poll_for_github_token_exits_on_expired() {
        let poller = SequencePoller::new(vec![DeviceTokenResponse {
            access_token: None,
            error: Some("expired_token".to_string()),
            error_description: None,
            interval: None,
        }]);
        let sleeper = RecordingSleeper::new();
        let device = DeviceCodeResponse {
            device_code: "dev".to_string(),
            user_code: "code".to_string(),
            verification_uri: "https://verify".to_string(),
            verification_uri_complete: None,
            expires_in: 30,
            interval: 2,
        };

        let err = poll_for_github_token(&poller, &sleeper, &device)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("expired"));
    }

    #[tokio::test]
    async fn poll_for_github_token_exits_on_denied() {
        let poller = SequencePoller::new(vec![DeviceTokenResponse {
            access_token: None,
            error: Some("access_denied".to_string()),
            error_description: None,
            interval: None,
        }]);
        let sleeper = RecordingSleeper::new();
        let device = DeviceCodeResponse {
            device_code: "dev".to_string(),
            user_code: "code".to_string(),
            verification_uri: "https://verify".to_string(),
            verification_uri_complete: None,
            expires_in: 30,
            interval: 2,
        };

        let err = poll_for_github_token(&poller, &sleeper, &device)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("denied"));
    }

    #[tokio::test]
    async fn poll_for_github_token_respects_timeout() {
        let poller = SequencePoller::new(vec![DeviceTokenResponse {
            access_token: None,
            error: Some("authorization_pending".to_string()),
            error_description: None,
            interval: None,
        }]);
        let sleeper = RecordingSleeper::new();
        let device = DeviceCodeResponse {
            device_code: "dev".to_string(),
            user_code: "code".to_string(),
            verification_uri: "https://verify".to_string(),
            verification_uri_complete: None,
            expires_in: 0,
            interval: 1,
        };

        let err = poll_for_github_token(&poller, &sleeper, &device)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn auth_store_path_prefers_explicit_path() {
        let path = auth_store_path(Some(PathBuf::from("/tmp/shipshape-auth.json"))).expect("path");
        assert_eq!(path, PathBuf::from("/tmp/shipshape-auth.json"));
    }

    #[test]
    fn auth_store_path_uses_env_override() {
        let _lock = env_lock();
        let _guard = EnvGuard::set("SHIPSHAPE_AUTH_PATH", Some("/tmp/shipshape-env.json"));
        let path = auth_store_path(None).expect("path");
        assert_eq!(path, PathBuf::from("/tmp/shipshape-env.json"));
    }

    #[test]
    fn auth_store_path_uses_xdg_config() {
        let _lock = env_lock();
        let _guard1 = EnvGuard::set("SHIPSHAPE_AUTH_PATH", None);
        let _guard2 = EnvGuard::set("XDG_CONFIG_HOME", Some("/tmp/xdg"));
        let path = auth_store_path(None).expect("path");
        assert_eq!(path, PathBuf::from("/tmp/xdg/shipshape/auth.json"));
    }

    #[test]
    fn auth_store_path_uses_home() {
        let _lock = env_lock();
        let _guard1 = EnvGuard::set("SHIPSHAPE_AUTH_PATH", None);
        let _guard2 = EnvGuard::set("XDG_CONFIG_HOME", None);
        let _guard3 = EnvGuard::set("HOME", Some("/tmp/home"));
        let path = auth_store_path(None).expect("path");
        assert_eq!(path, PathBuf::from("/tmp/home/.config/shipshape/auth.json"));
    }

    #[test]
    fn auth_store_path_errors_when_unset() {
        let _lock = env_lock();
        let _guard1 = EnvGuard::set("SHIPSHAPE_AUTH_PATH", None);
        let _guard2 = EnvGuard::set("XDG_CONFIG_HOME", None);
        let _guard3 = EnvGuard::set("HOME", None);
        let err = auth_store_path(None).unwrap_err();
        assert!(err.to_string().contains("auth storage"));
    }

    #[tokio::test]
    async fn write_auth_session_persists_json() {
        let root = std::env::temp_dir().join("shipshape_cli_auth_store");
        let path = root.join("auth.json");
        let session = StoredAuthSession {
            server_url: "http://localhost:8080".to_string(),
            token: "shipshape-token".to_string(),
            user: AuthUser {
                id: "usr-1".to_string(),
                login: "pilot".to_string(),
                github_id: "42".to_string(),
            },
        };

        write_auth_session(&path, &session)
            .await
            .expect("write auth");
        let contents = tokio::fs::read_to_string(&path).await.expect("read auth");
        let parsed: StoredAuthSession = serde_json::from_str(&contents).expect("parse auth");
        assert_eq!(parsed, session);
    }

    #[tokio::test]
    async fn run_login_happy_path() {
        let config = AuthConfigResponse {
            client_id: "client-123".to_string(),
            authorize_url: "https://github.com/login/oauth/authorize".to_string(),
            scopes: vec!["read:user".to_string(), "repo".to_string()],
        };
        let device = DeviceCodeResponse {
            device_code: "device-123".to_string(),
            user_code: "ABCD-1234".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            verification_uri_complete: Some(
                "https://github.com/login/device?code=ABCD-1234".to_string(),
            ),
            expires_in: 600,
            interval: 1,
        };
        let session = AuthGithubResponse {
            token: "shipshape-token".to_string(),
            user: AuthUser {
                id: "usr-1".to_string(),
                login: "pilot".to_string(),
                github_id: "42".to_string(),
            },
        };
        let client = TestLoginClient::new(
            config,
            device,
            vec![DeviceTokenResponse {
                access_token: Some("gh-token".to_string()),
                error: None,
                error_description: None,
                interval: None,
            }],
            session,
        );
        let sleeper = RecordingSleeper::new();
        let path = std::env::temp_dir().join("shipshape_cli_login_auth.json");
        let args = LoginArgs {
            server_url: "http://localhost:8080".to_string(),
            auth_path: Some(path.clone()),
        };

        run_login_with(args, &client, &sleeper)
            .await
            .expect("run login");
        let contents = tokio::fs::read_to_string(&path).await.expect("read auth");
        let parsed: StoredAuthSession = serde_json::from_str(&contents).expect("parse auth");
        assert_eq!(parsed.token, "shipshape-token");
        assert_eq!(parsed.user.login, "pilot");
        assert!(sleeper.durations().is_empty());
    }
}
