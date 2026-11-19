//! tephra: a small self-hosted server for TRMNL devices. See DESIGN.md.

pub mod apps;
pub mod auth;
pub mod config;
pub mod control_api;
pub mod db;
pub mod dev;
pub mod device_api;
pub mod display;
pub mod luatime;
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
    // Everything here needs a login or token while the lock is on (see auth.rs).
    let guarded = Router::new()
        .merge(web::router())
        .route("/agent", axum::routing::get(auth::agent_brief))
        .nest("/api/v1", control_api::router().merge(auth::router()))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth::guard));
    // ...and these never do: what devices call, and signing in.
    Router::new()
        .merge(guarded)
        .merge(device_api::router())
        .merge(auth::public_router())
        .merge(web::public_router())
        .with_state(state)
}

pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let db = Db::open(&cfg.db_path())?;
    let apps = apps::Apps::load(&cfg.apps_dir, &model::stored_apps(&db.conn())?);
    let listen = cfg.listen;
    let state = Arc::new(AppState { cfg, db, apps });
    tokio::spawn(display::scheduler(state.clone()));
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!("listening on http://{listen}");
    // The peer address lets the developer API answer only this machine.
    axum::serve(listener, router(state).into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
