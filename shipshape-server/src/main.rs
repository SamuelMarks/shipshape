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

#[cfg(not(test))]
use actix_cors::Cors;
#[cfg(not(test))]
use actix_web::{App, HttpServer, middleware::Logger, web};
use reqwest::Client;

use crate::crypto::TokenCipher;
use crate::db::init_pool;
use crate::error::ServerResult;
#[cfg(not(test))]
use crate::routes::configure_routes;
use crate::routes::{AppState, AuthConfig, seed_diff_store};
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

#[cfg(not(test))]
fn main() -> ServerResult<()> {
    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    // Initialize blocking HTTP clients inside `init_app_state` before starting the Actix runtime.
    let state = init_app_state()?;
    let (listen_addr, listen_port) = resolve_listen_config()?;

    log::info!("Starting ShipShape server on {listen_addr}:{listen_port}");

    actix_web::rt::System::new().block_on(async move {
        HttpServer::new(move || {
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
        .bind((listen_addr, listen_port))?
        .run()
        .await?;
        Ok(())
    })
}

#[cfg(test)]
fn main() {}

#[cfg(test)]
mod tests {
    use super::{init_app_state, main, resolve_listen_config};
    use crate::db::TestDatabase;
    use crate::error::ServerResult;

    #[test]
    fn resolve_listen_config_and_init_app_state_succeed() -> ServerResult<()> {
        main();
        let _guard = crate::db::db_env_lock()?;
        let mut test_db = TestDatabase::new()?;
        let _pool = test_db.pool()?;

        let prev_db = std::env::var("DATABASE_URL").ok();
        let prev_key = std::env::var("SHIPSHAPE_TOKEN_KEY").ok();
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
            std::env::set_var("SHIPSHAPE_PORT", "9090");
        }

        let (host, port) = resolve_listen_config()?;
        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 9090);

        let _state = init_app_state()?;

        crate::db::restore_env_var("DATABASE_URL", prev_db);
        crate::db::restore_env_var("SHIPSHAPE_TOKEN_KEY", prev_key);
        crate::db::restore_env_var("SHIPSHAPE_PORT", prev_port);
        Ok(())
    }
}
