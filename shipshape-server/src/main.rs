#![deny(missing_docs)]
//! ShipShape server executable.
//!
//! Hosts HTTP endpoints for voyage coordination and refit workflows.

mod db;
mod crypto;
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

#[cfg(not(test))]
use crate::db::init_pool;
#[cfg(not(test))]
use crate::crypto::TokenCipher;
#[cfg(not(test))]
use crate::routes::{
    AppState, AuthConfig, auth_config, auth_github, auth_github_token, auth_me, batch_runs,
    control_options, control_queue, dashboard, diffs, openapi_json, vessel_diagnostics,
    vessel_refit, vessel_workflow, voyage_board, voyage_launch,
};
#[cfg(not(test))]
use crate::workflows::WorkflowService;

#[cfg(not(test))]
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let pool = init_pool();
    let workflow = WorkflowService::from_env();
    let auth = AuthConfig::from_env();
    let token_cipher =
        TokenCipher::from_env().expect("SHIPSHAPE_TOKEN_KEYS must be set for token encryption");
    let state = web::Data::new(AppState {
        pool,
        workflow,
        auth,
        token_cipher,
    });
    let origins = std::env::var("SHIPSHAPE_UI_ORIGINS")
        .unwrap_or_else(|_| "http://127.0.0.1:4200,http://localhost:4200".to_string());
    let allowed_origins: Vec<String> = origins
        .split(',')
        .map(|value| value.trim())
        .filter(|origin| !origin.is_empty())
        .map(String::from)
        .collect();

    HttpServer::new(move || {
        let mut cors = Cors::default()
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec![header::AUTHORIZATION, header::CONTENT_TYPE])
            .max_age(3600);
        for origin in &allowed_origins {
            cors = cors.allowed_origin(origin);
        }
        App::new()
            .wrap(cors)
            .app_data(state.clone())
            .service(auth_config)
            .service(auth_github)
            .service(auth_github_token)
            .service(auth_me)
            .service(dashboard)
            .service(batch_runs)
            .service(diffs)
            .service(control_options)
            .service(control_queue)
            .service(voyage_board)
            .service(vessel_diagnostics)
            .service(vessel_refit)
            .service(vessel_workflow)
            .service(voyage_launch)
            .service(openapi_json)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}

#[cfg(test)]
fn main() {}
