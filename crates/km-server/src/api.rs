//! HTTP + WebSocket API and static file serving.
//!
//! Endpoints:
//! - `GET  /api/telemetry` — latest telemetry snapshot (JSON).
//! - `POST /api/command`   — submit a [`Command`] (JSON).
//! - `GET  /api/ws`        — WebSocket: streams telemetry, accepts commands.
//! - `GET  /*`             — serves the built frontend from `frontend/dist`.

use std::path::PathBuf;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::state::{AppState, Command, Telemetry};

/// Build the Axum router. `frontend_dist` is served at the root as a fallback
/// so client-side routing works; during development the React dev server runs
/// separately and reaches the API via permissive CORS.
pub fn router(state: AppState, frontend_dist: PathBuf) -> Router {
    let api = Router::new()
        .route("/telemetry", get(get_telemetry))
        .route("/command", post(post_command))
        .route("/ws", get(ws_handler));

    Router::new()
        .nest("/api", api)
        .fallback_service(ServeDir::new(frontend_dist))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Latest telemetry snapshot.
async fn get_telemetry(State(state): State<AppState>) -> Json<Telemetry> {
    Json(state.telemetry.borrow().clone())
}

/// Submit a single command to the control loop.
async fn post_command(
    State(state): State<AppState>,
    Json(cmd): Json<Command>,
) -> impl IntoResponse {
    match state.commands.send(cmd).await {
        Ok(()) => StatusCode::ACCEPTED,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// Upgrade to a WebSocket that pushes telemetry and accepts commands.
async fn ws_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut telemetry = state.telemetry.clone();

    // Send the current snapshot immediately so the UI isn't blank until the
    // next change. Clone to an owned value first so the (non-Send) watch Ref
    // isn't held across the await.
    let snapshot = telemetry.borrow_and_update().clone();
    if send_telemetry(&mut socket, &snapshot).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            // Telemetry changed → push it to the client.
            changed = telemetry.changed() => {
                if changed.is_err() {
                    break; // control loop dropped the sender
                }
                let snapshot = telemetry.borrow_and_update().clone();
                if send_telemetry(&mut socket, &snapshot).await.is_err() {
                    break;
                }
            }
            // Inbound message → parse as a command and forward it.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<Command>(&text) {
                            Ok(cmd) => { let _ = state.commands.send(cmd).await; }
                            Err(e) => tracing::warn!(error = %e, "bad command over ws"),
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        tracing::warn!(error = %e, "ws error");
                        break;
                    }
                    _ => {} // ignore binary/ping/pong
                }
            }
        }
    }
}

async fn send_telemetry(socket: &mut WebSocket, t: &Telemetry) -> anyhow::Result<()> {
    let json = serde_json::to_string(t)?;
    socket.send(Message::Text(json)).await?;
    Ok(())
}
