//! Shared types and channels connecting the web API to the control loop.
//!
//! Data flow:
//! - The REST command handlers send [`Command`]s into an mpsc channel; the
//!   control loop drains it at the top of each tick.
//! - The control loop publishes [`Telemetry`] on a `watch` channel; every SSE
//!   stream and the REST snapshot endpoint read the latest value.

use km_control::VesselState;
use serde::Serialize;
use tokio::sync::{mpsc, watch};

/// A command from the operator to the control loop. Internal to the server —
/// per AD-0001 the HTTP surface is one REST endpoint per action, and handlers
/// construct these directly.
#[derive(Debug, Clone)]
pub enum Command {
    /// Engage the hover controller (resets the controller's integrators first).
    Arm,
    /// Disengage: cut throttle and stop commanding.
    Disarm,
    /// Set the altitude setpoint, meters above the surface.
    SetTargetAltitude { altitude: f64 },
}

/// A snapshot the control loop publishes every tick for observers.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Telemetry {
    /// Whether the controller is currently engaged.
    pub armed: bool,
    /// Last commanded throttle, `[0, 1]`.
    pub throttle: f64,
    /// Current altitude setpoint, meters.
    pub target_altitude: f64,
    /// Latest sampled vessel state.
    pub state: VesselState,
    /// Seconds since the control loop started.
    pub t: f64,
    /// Which plant is driving the loop: `"sim"` or `"krpc"`.
    pub source: &'static str,
}

/// Handle shared with every Axum request via application state.
#[derive(Clone)]
pub struct AppState {
    /// Send operator commands into the control loop.
    pub commands: mpsc::Sender<Command>,
    /// Read the latest telemetry published by the control loop.
    pub telemetry: watch::Receiver<Telemetry>,
}

/// Create the channels that wire the API and the control loop together.
///
/// Returns the [`AppState`] for the web server and the receiving/sending ends
/// the control loop owns.
pub fn channels() -> (AppState, mpsc::Receiver<Command>, watch::Sender<Telemetry>) {
    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let (telem_tx, telem_rx) = watch::channel(Telemetry::default());
    let app = AppState {
        commands: cmd_tx,
        telemetry: telem_rx,
    };
    (app, cmd_rx, telem_tx)
}
