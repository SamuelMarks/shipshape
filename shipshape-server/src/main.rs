#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
//! ShipShape server executable.
//!
//! Hosts HTTP endpoints for voyage coordination, authentication, interactive
//! diff review, and GitHub/GitLab refit workflows.

mod crypto;
mod db;
mod error;
mod models;
mod openapi;
mod routes;
mod schema;
mod workflows;

use std::str::FromStr;

use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware::Logger, web};
use reqwest::Client;

use crate::crypto::TokenCipher;
use crate::db::init_pool;
use crate::error::ServerResult;
use crate::routes::{AppState, AuthConfig, configure_routes, seed_diff_store};
use crate::workflows::WorkflowService;

/// Resolve the server bind address and TCP port from environment variables.
pub fn resolve_listen_config() -> ServerResult<(String, u16)> {
    let listen_addr = std::env::var("SHIPSHAPE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let listen_port = std::env::var("SHIPSHAPE_PORT")
        .map(|port| u16::from_str(&port))
        .unwrap_or(Ok(8080))?;
    Ok((listen_addr, listen_port))
}

/// Initialize [`AppState`] from environment variables and database configuration.
pub fn init_app_state() -> ServerResult<AppState> {
    let pool = init_pool()?;
    let workflow = WorkflowService::from_env();
    let auth = AuthConfig::from_env();
    let token_cipher = TokenCipher::from_env()?;
    let http_client = Client::new();
    let diff_store = seed_diff_store();
    Ok(AppState {
        pool,
        workflow,
        auth,
        token_cipher,
        http_client,
        diff_store,
    })
}

/// Build an [`HttpServer`] bound to `(listen_addr, listen_port)`.
///
/// # Arguments
/// * `state` - Shared application state.
/// * `listen_addr` - Bind host or IP address.
/// * `listen_port` - Bind TCP port.
pub fn bind_http_server(
    state: AppState,
    listen_addr: &str,
    listen_port: u16,
) -> ServerResult<actix_web::dev::Server> {
    let server = HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header();
        App::new()
            .wrap(Logger::default())
            .wrap(cors)
            .app_data(web::Data::new(state.clone()))
            .configure(configure_routes)
    })
    .workers(1)
    .disable_signals()
    .bind((listen_addr, listen_port))?
    .run();
    Ok(server)
}

/// Entry point for the ShipShape server executable.
fn main() -> ServerResult<()> {
    dotenvy::dotenv().ok();
    let _ = env_logger::try_init_from_env(env_logger::Env::default().default_filter_or("info"));
    let state = init_app_state()?;
    let (listen_addr, listen_port) = resolve_listen_config()?;
    log::info!("Starting ShipShape server on {listen_addr}:{listen_port}");
    actix_web::rt::System::new().block_on(async move {
        let server = bind_http_server(state, &listen_addr, listen_port)?;
        #[cfg(test)]
        {
            actix_web::rt::spawn(server.handle().stop(false));
        }
        server.await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::{init_app_state, main, resolve_listen_config};
    use crate::db::TestDatabase;
    use crate::error::ServerResult;

    #[test]
    fn resolve_listen_config_and_init_app_state_succeed() -> ServerResult<()> {
        let _guard = crate::db::db_env_lock()?;
        let mut test_db = TestDatabase::new()?;
        let _pool = test_db.pool()?;

        let prev_db = std::env::var("DATABASE_URL").ok();
        let prev_key = std::env::var("SHIPSHAPE_TOKEN_KEY").ok();
        let prev_host = std::env::var("SHIPSHAPE_HOST").ok();
        let prev_port = std::env::var("SHIPSHAPE_PORT").ok();

        unsafe {
            std::env::remove_var("SHIPSHAPE_PORT");
        }
        let (_, default_port) = resolve_listen_config()?;
        assert_eq!(default_port, 8080);

        unsafe {
            std::env::set_var("DATABASE_URL", test_db.database_url());
            std::env::set_var(
                "SHIPSHAPE_TOKEN_KEY",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            );
            std::env::set_var("SHIPSHAPE_HOST", "127.0.0.1");
            std::env::set_var("SHIPSHAPE_PORT", "0");
        }

        let (host, port) = resolve_listen_config()?;
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 0);

        let _state = init_app_state()?;
        main()?;

        crate::db::restore_env_var("DATABASE_URL", prev_db);
        crate::db::restore_env_var("SHIPSHAPE_TOKEN_KEY", prev_key);
        crate::db::restore_env_var("SHIPSHAPE_HOST", prev_host);
        crate::db::restore_env_var("SHIPSHAPE_PORT", prev_port);
        Ok(())
    }
}
