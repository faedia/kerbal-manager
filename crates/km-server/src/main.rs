//! Kerbal Manager server: runs the flight-control loop and the web API.
//!
//! By default it flies the offline [`SimPlant`], so you can run the whole
//! stack — server, WebSocket telemetry, web UI — with KSP closed. Build with
//! `--features krpc` and set `KM_KRPC=1` to connect to a live vessel instead.

mod api;
mod control_loop;
mod plant;
mod state;

#[cfg(feature = "krpc")]
mod krpc_plant;

use std::net::SocketAddr;
use std::path::PathBuf;

use plant::SimPlant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let (app_state, commands, telemetry) = state::channels();

    // Pick the plant. The control loop is generic, so each branch spawns the
    // same loop monomorphized for its plant type.
    let dt = 1.0 / control_loop::CONTROL_HZ;
    let use_krpc = cfg!(feature = "krpc") && std::env::var("KM_KRPC").is_ok();

    if use_krpc {
        #[cfg(feature = "krpc")]
        {
            let cfg = krpc_plant::KrpcConfig::default();
            tracing::info!(host = %cfg.host, "connecting to kRPC");
            let plant = krpc_plant::KrpcPlant::connect(&cfg).await?;
            tokio::spawn(control_loop::run(plant, commands, telemetry));
        }
    } else {
        let plant = SimPlant::new(dt);
        tokio::spawn(control_loop::run(plant, commands, telemetry));
    }

    // Serve the API and the built frontend.
    let frontend_dist = std::env::var("KM_FRONTEND_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("frontend/dist"));
    let app = api::router(app_state, frontend_dist);

    let addr: SocketAddr = std::env::var("KM_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;
    tracing::info!(%addr, "serving HTTP + WebSocket");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,km_server=debug")),
        )
        .init();
}
