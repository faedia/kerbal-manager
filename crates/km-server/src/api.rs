//! HTTP API and static file serving.
//!
//! Per AD-0001 (docs/decisions/0001-rest-commands-sse-telemetry.md), the
//! surface is split by traffic type:
//! discrete commands are REST endpoints with validation and status codes;
//! telemetry is streamed server → client over SSE. State truth lives in the
//! telemetry stream — `202 Accepted` means *validated and queued*, and the UI
//! observes the effect via telemetry on the next control-loop tick.
//!
//! - `GET  /api/telemetry`              — latest snapshot (JSON).
//! - `GET  /api/telemetry/stream`       — SSE stream of snapshots.
//! - `POST /api/vessel/arm`             — engage the hover controller.
//! - `POST /api/vessel/disarm`          — disengage; throttle is cut.
//! - `PUT  /api/vessel/target-altitude` — set the altitude setpoint.
//! - `GET  /*`                          — serves the built frontend from `frontend/dist`.

use std::convert::Infallible;
use std::path::PathBuf;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{Stream, StreamExt};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::state::{AppState, Command, Telemetry};

/// Build the Axum router. `frontend_dist` is served at the root as a fallback
/// so client-side routing works; during development the React dev server runs
/// separately and reaches the API via permissive CORS.
pub fn router(state: AppState, frontend_dist: PathBuf) -> Router {
    let api = Router::new()
        .route("/telemetry", get(get_telemetry))
        .route("/telemetry/stream", get(telemetry_stream))
        .route("/vessel/arm", post(arm))
        .route("/vessel/disarm", post(disarm))
        .route("/vessel/target-altitude", put(set_target_altitude));

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

/// SSE stream of telemetry snapshots.
///
/// The current snapshot is sent immediately on connect (`WatchStream` yields
/// the initial value), then one event per control-loop publish the client
/// keeps up with — the watch channel coalesces under backpressure, so slow
/// clients skip intermediate snapshots and always get the newest.
///
/// NOTE (AD-0001 caveat): if a compression layer is ever added to the router,
/// exclude this route — buffered `text/event-stream` stalls silently.
async fn telemetry_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = WatchStream::new(state.telemetry.clone()).map(|t: Telemetry| {
        // Named event so future event types (alerts, mode changes) can share
        // the stream without breaking existing listeners.
        Ok(Event::default()
            .event("telemetry")
            .json_data(&t)
            // Telemetry is plain structs of numbers; serialization can't
            // realistically fail, but never panic a handler over it.
            .unwrap_or_else(|_| Event::default().event("error").data("serialization failed")))
    });

    // At 50 Hz the data is its own heartbeat, but keep-alives stop proxies
    // from timing out a paused/idle stream.
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Engage the hover controller (resets controller integrators first).
async fn arm(State(state): State<AppState>) -> Response {
    queue(&state, Command::Arm).await
}

/// Disengage: the loop cuts the throttle and goes hands-off.
async fn disarm(State(state): State<AppState>) -> Response {
    queue(&state, Command::Disarm).await
}

#[derive(Debug, Deserialize)]
struct TargetAltitudeBody {
    altitude: f64,
}

/// Set the altitude setpoint, meters above the surface.
async fn set_target_altitude(
    State(state): State<AppState>,
    Json(body): Json<TargetAltitudeBody>,
) -> Response {
    // JSON can't encode NaN/Infinity, but serde can still produce them from
    // out-of-range literals (e.g. 1e999), so check anyway.
    if !body.altitude.is_finite() || body.altitude < 0.0 {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "altitude must be a finite number >= 0" })),
        )
            .into_response();
    }
    queue(
        &state,
        Command::SetTargetAltitude {
            altitude: body.altitude,
        },
    )
    .await
}

/// Queue a command for the control loop, mapped onto the API conventions:
/// `202 {"queued":true}` on success, `503` if the loop is gone.
async fn queue(state: &AppState, cmd: Command) -> Response {
    match state.commands.send(cmd).await {
        Ok(()) => (StatusCode::ACCEPTED, Json(json!({ "queued": true }))).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "control loop is not running" })),
        )
            .into_response(),
    }
}
