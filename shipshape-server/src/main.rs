#![deny(missing_docs)]
//! ShipShape server executable.
//!
//! Hosts HTTP endpoints for voyage coordination and refit workflows.

mod crypto;
mod db;
mod models;
mod openapi;
mod routes;
mod schema;
mod workflows;

#[cfg(not(test))]
use actix_cors::Cors;
#[cfg(not(test))]
use actix_web::{App, HttpServer, http::header, web};
#[cfg(not(test))]
use dotenvy::dotenv;

#[allow(unused_imports)]
use std::str::FromStr;

#[cfg(not(test))]
use crate::crypto::TokenCipher;
#[cfg(not(test))]
use crate::db::init_pool;
#[cfg(not(test))]
use crate::routes::{
    AppState, AuthConfig, auth_config, auth_github, auth_github_token, auth_me, batch_runs,
    control_options, control_queue, dashboard, diff_update, diffs, openapi_json, seed_diff_store,
    vessel_diagnostics, vessel_refit, vessel_workflow, voyage_board, voyage_launch,
};
#[cfg(not(test))]
use crate::workflows::WorkflowService;

#[cfg(not(test))]
fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let pool = init_pool();

    // Initialize blocking clients synchronously before the async runtime starts.
    // This prevents the panic caused by creating a `reqwest::blocking::Client`
    // inside the Actix runtime.
    let workflow = WorkflowService::from_env();
    let auth = AuthConfig::from_env();
    let token_cipher =
        TokenCipher::from_env().expect("SHIPSHAPE_TOKEN_KEYS must be set for token encryption");

    let state = web::Data::new(AppState {
        pool,
        workflow,
        auth,
        token_cipher,
        diff_store: seed_diff_store(),
    });

    let origins = std::env::var("SHIPSHAPE_UI_ORIGINS")
        .unwrap_or_else(|_| "http://127.0.0.1:4200,http://localhost:4200".to_string());
    let allowed_origins: Vec<String> = origins
        .split(',')
        .map(|value| value.trim())
        .filter(|origin| !origin.is_empty())
        .map(String::from)
        .collect();

    let listen_addr = std::env::var("SHIPSHAPE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listen_port =
        u16::from_str(&std::env::var("SHIPSHAPE_PORT").unwrap_or_else(|_| "8080".to_string()))
            .expect("SHIPSHAPE_PORT must be a u16 number");
    let err_msg = format!("Can't bind {}:{}", &listen_addr, listen_port);

    // Manually start the Actix system
    actix_web::rt::System::new().block_on(async move {
        HttpServer::new(move || {
            let mut cors = Cors::default()
                .allowed_methods(vec!["GET", "POST", "OPTIONS"])
                .allowed_headers(vec![header::AUTHORIZATION, header::CONTENT_TYPE])
                .max_age(3600);
            for origin in &allowed_origins {
                cors = cors.allowed_origin(origin);
            }
            App::new()
                .wrap(actix_web::middleware::Logger::default())
                .wrap(cors)
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
                .service(openapi_json)
        })
        .bind((listen_addr, listen_port))
        .expect(&err_msg)
        .run()
        .await
    })
}

#[cfg(test)]
fn main() {}
