//! The real-time control loop.
//!
//! Runs at a fixed rate, independent of any plant. Each tick it:
//! 1. drains pending operator [`Command`]s,
//! 2. samples the [`Plant`] for the current [`VesselState`],
//! 3. while armed, runs the [`HoverController`] and writes the throttle back,
//! 4. publishes a [`Telemetry`] snapshot — on *every* tick, including plant
//!    errors, so observers never see a stale "armed" state.
//!
//! The loop only commands the throttle while armed (plus a single
//! zero-throttle write on the armed → disarmed transition), so a disarmed
//! vessel is fully hands-off and can be flown manually.
//!
//! It is generic over `P: Plant`, so the same code drives the simulator or a
//! live kRPC vessel with no branching in the loop body. It exits when the
//! command channel closes (the API side has shut down).

use std::time::{Duration, Instant};

use km_control::{HoverConfig, HoverController, VesselState};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, watch};
use tokio::time::{interval, MissedTickBehavior};

use crate::plant::Plant;
use crate::state::{Command, Telemetry};

/// Control loop tick rate.
pub const CONTROL_HZ: f64 = 50.0;

/// Upper bound on the integration step fed to the controller. If the loop
/// stalls (game pause, debugger, kRPC hiccup) the PID shouldn't integrate one
/// giant step across the gap.
const MAX_DT: f64 = 0.25;

/// Run the loop until the command channel closes. Generic over the plant.
pub async fn run<P: Plant>(
    mut plant: P,
    mut commands: mpsc::Receiver<Command>,
    telemetry: watch::Sender<Telemetry>,
) {
    let mut ticker = interval(Duration::from_secs_f64(1.0 / CONTROL_HZ));
    // If a tick is late (e.g. the game stalls), don't try to catch up in a burst.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut controller = HoverController::new(HoverConfig::default(), 100.0);
    let mut armed = false;
    // Set when `armed` drops; cleared once a zero-throttle write succeeds.
    let mut cut_throttle_pending = false;
    // Last successfully sampled state; republished on sample errors so the
    // telemetry stream stays truthful about `armed` even when the plant is down.
    let mut last_state = VesselState::default();
    let started = Instant::now();
    let mut last_tick = started;

    let source = plant.source();
    tracing::info!(source, hz = CONTROL_HZ, "control loop started");

    'ticks: loop {
        ticker.tick().await;

        // Integrate with the *measured* tick period, not the nominal one: over
        // kRPC a tick can easily overrun 1/CONTROL_HZ, and the PID integral
        // and derivative are only correct when dt is real elapsed time.
        let now = Instant::now();
        let dt = (now - last_tick).as_secs_f64().min(MAX_DT);
        last_tick = now;

        // 1. Apply queued commands; exit when the API side has gone away.
        loop {
            match commands.try_recv() {
                Ok(Command::Arm) => {
                    controller.reset();
                    armed = true;
                    cut_throttle_pending = false;
                    tracing::info!("armed");
                }
                Ok(Command::Disarm) => {
                    disarm(&mut armed, &mut cut_throttle_pending);
                    tracing::info!("disarmed");
                }
                Ok(Command::SetTargetAltitude { altitude }) => {
                    controller.set_target_altitude(altitude);
                    tracing::info!(altitude, "target altitude set");
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break 'ticks,
            }
        }

        // 2. Sample the plant. On failure, disarm but fall through so step 4
        // still publishes (stale state, but truthful `armed`).
        match plant.sample().await {
            Ok(s) => last_state = s,
            Err(e) => {
                tracing::error!(error = %e, "plant sample failed; disarming");
                disarm(&mut armed, &mut cut_throttle_pending);
            }
        }

        // 3. While armed, run the controller and command the throttle. While
        // disarmed the plant is left alone so it can be flown manually.
        let mut throttle = 0.0;
        if armed {
            throttle = controller.update(&last_state, dt).throttle;
            if let Err(e) = plant.set_throttle(throttle).await {
                tracing::error!(error = %e, "set_throttle failed; disarming");
                disarm(&mut armed, &mut cut_throttle_pending);
                throttle = 0.0;
            }
        }
        // One-shot zero-throttle on the armed → disarmed transition, retried
        // each tick until the write goes through.
        if cut_throttle_pending && plant.set_throttle(0.0).await.is_ok() {
            cut_throttle_pending = false;
        }

        // 4. Publish telemetry (ignore send error: just means no receivers yet).
        let _ = telemetry.send(Telemetry {
            armed,
            throttle,
            target_altitude: controller.target_altitude,
            state: last_state,
            t: started.elapsed().as_secs_f64(),
            source,
        });
    }

    tracing::info!("command channel closed; control loop exiting");
}

/// Drop out of the armed state, scheduling a final zero-throttle write. A
/// no-op when already disarmed (no spurious throttle writes).
fn disarm(armed: &mut bool, cut_throttle_pending: &mut bool) {
    if *armed {
        *armed = false;
        *cut_throttle_pending = true;
    }
}
