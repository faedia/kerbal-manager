//! Kerbal Manager server: runs the flight-control loop and the web API.
//!
//! By default it flies the offline [`SimPlant`], so you can run the whole
//! stack — server, SSE telemetry, web UI — with KSP closed. Build with
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
    let use_krpc = cfg!(feature = "krpc") && env_flag("KM_KRPC");

    #[cfg(not(feature = "krpc"))]
    if env_flag("KM_KRPC") {
        tracing::warn!(
            "KM_KRPC is set but this binary was built without the `krpc` feature; \
             falling back to the simulator (rebuild with --features krpc)"
        );
    }

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

    // Default to loopback: the API is unauthenticated and can arm the vessel,
    // so exposing it beyond this machine must be an explicit choice
    // (KM_BIND=0.0.0.0:8080).
    let addr: SocketAddr = std::env::var("KM_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()?;
    tracing::info!(%addr, "serving HTTP API (REST commands + SSE telemetry)");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Boolean env-var convention: unset, empty, `0`, `false`, `no`, and `off`
/// (case-insensitive) are false; anything else is true. So `KM_KRPC=0`
/// actually disables the live link instead of surprisingly enabling it.
fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        ),
        Err(_) => false,
    }
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
