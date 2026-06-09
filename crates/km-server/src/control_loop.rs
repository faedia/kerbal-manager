//! The real-time control loop.
//!
//! Runs at a fixed rate, independent of any plant. Each tick it:
//! 1. drains pending operator [`Command`]s,
//! 2. samples the [`Plant`] for the current [`VesselState`],
//! 3. if armed, runs the [`HoverController`] and writes the throttle back,
//! 4. publishes a [`Telemetry`] snapshot for observers.
//!
//! It is generic over `P: Plant`, so the same code drives the simulator or a
//! live kRPC vessel with no branching in the loop body.

use std::time::Duration;

use km_control::{HoverConfig, HoverController};
use tokio::sync::{mpsc, watch};
use tokio::time::{interval, MissedTickBehavior};

use crate::plant::Plant;
use crate::state::{Command, Telemetry};

/// Control loop tick rate.
pub const CONTROL_HZ: f64 = 50.0;

/// Run the loop until the command channel closes. Generic over the plant.
pub async fn run<P: Plant>(
    mut plant: P,
    mut commands: mpsc::Receiver<Command>,
    telemetry: watch::Sender<Telemetry>,
) {
    let dt = 1.0 / CONTROL_HZ;
    let mut ticker = interval(Duration::from_secs_f64(dt));
    // If a tick is late (e.g. the game stalls), don't try to catch up in a burst.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut controller = HoverController::new(HoverConfig::default(), 100.0);
    let mut armed = false;
    let mut elapsed = 0.0_f64;

    let source = plant.source();
    tracing::info!(source, hz = CONTROL_HZ, "control loop started");

    loop {
        ticker.tick().await;
        elapsed += dt;

        // 1. Apply any queued commands.
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                Command::Arm => {
                    controller.reset();
                    armed = true;
                    tracing::info!("armed");
                }
                Command::Disarm => {
                    armed = false;
                    tracing::info!("disarmed");
                }
                Command::SetTargetAltitude { altitude } => {
                    controller.set_target_altitude(altitude);
                    tracing::info!(altitude, "target altitude set");
                }
            }
        }

        // 2. Sample the plant.
        let state = match plant.sample().await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "plant sample failed; disarming");
                armed = false;
                continue;
            }
        };

        // 3. Compute and apply throttle.
        let throttle = if armed {
            controller.update(&state, dt).throttle
        } else {
            0.0
        };
        if let Err(e) = plant.set_throttle(throttle).await {
            tracing::error!(error = %e, "set_throttle failed; disarming");
            armed = false;
        }

        // 4. Publish telemetry (ignore send error: just means no receivers yet).
        let _ = telemetry.send(Telemetry {
            armed,
            throttle,
            target_altitude: controller.target_altitude,
            state,
            t: elapsed,
            source,
        });
    }
}
