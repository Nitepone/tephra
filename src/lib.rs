//! tephra: a small self-hosted server for TRMNL devices. See DESIGN.md.

pub mod apps;
pub mod config;
pub mod control_api;
pub mod db;
pub mod device_api;
pub mod display;
pub mod model;
pub mod render;
pub mod screen;
pub mod web;

use axum::Router;
use config::Config;
use db::Db;
use std::sync::Arc;

/// What every request handler can reach.
pub struct AppState {
    pub cfg: Config,
    pub db: Db,
    pub apps: apps::Apps,
}

pub type SharedState = Arc<AppState>;

pub fn router(state: SharedState) -> Router {
    Router::new()
        .merge(web::router())
        .merge(device_api::router())
        .nest("/api/v1", control_api::router())
        .with_state(state)
}

pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let db = Db::open(&cfg.db_path())?;
    let apps = apps::Apps::load(&cfg.apps_dir);
    let listen = cfg.listen;
    let state = Arc::new(AppState { cfg, db, apps });
    tokio::spawn(display::scheduler(state.clone()));
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!("listening on http://{listen}");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
