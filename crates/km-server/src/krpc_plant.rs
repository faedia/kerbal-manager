//! Live Kerbal Space Program plant, backed by the `krpc-client` crate.
//!
//! Compiled only under the `krpc` feature. It connects to the kRPC server
//! running inside KSP, samples the active vessel's surface-relative flight
//! state, and writes the throttle back.
//!
//! NOTE: the exact `krpc-client` method names below follow the documented kRPC
//! `SpaceCenter` service. Verify against your installed `krpc-client` version
//! the first time you fly — this module is intentionally the only place that
//! touches the live API, so any drift is contained here.

use std::sync::Arc;

use anyhow::Context;
use km_control::VesselState;
use krpc_client::services::space_center::{ReferenceFrame, SpaceCenter, Vessel};
use krpc_client::Client;

use crate::plant::Plant;

/// Connection parameters for the in-game kRPC server.
#[derive(Debug, Clone)]
pub struct KrpcConfig {
    pub host: String,
    pub rpc_port: u16,
    pub stream_port: u16,
}

impl Default for KrpcConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            rpc_port: 50000,
            stream_port: 50001,
        }
    }
}

/// A [`Plant`] that drives the active vessel over kRPC.
pub struct KrpcPlant {
    sc: SpaceCenter,
    vessel: Vessel,
    /// The orbited body's **rotating** reference frame. Velocities reported in
    /// this frame are surface-relative — this is what makes `vertical_speed`
    /// the true climb rate. (See the note in `sample`.)
    body_frame: ReferenceFrame,
    /// Surface gravity of the orbited body, m/s². Constant for a given body, so
    /// we read it once at connect rather than every tick.
    surface_gravity: f64,
}

impl KrpcPlant {
    /// Connect to KSP and grab the active vessel.
    pub async fn connect(cfg: &KrpcConfig) -> anyhow::Result<Self> {
        let client: Arc<Client> = Client::new(
            "kerbal-manager",
            &cfg.host,
            cfg.rpc_port,
            cfg.stream_port,
        )
        .await
        .context("connecting to kRPC server (is KSP running with kRPC started?)")?;

        let sc = SpaceCenter::new(client.clone());
        let vessel = sc
            .get_active_vessel()
            .await
            .context("no active vessel — switch to a flight scene")?;

        // Cache the orbited body's rotating reference frame and surface gravity.
        let body = vessel.get_orbit().await?.get_body().await?;
        let body_frame = body.get_reference_frame().await?;
        let surface_gravity = body.get_surface_gravity().await? as f64;

        Ok(Self {
            sc,
            vessel,
            body_frame,
            surface_gravity,
        })
    }
}

impl Plant for KrpcPlant {
    fn source(&self) -> &'static str {
        "krpc"
    }

    async fn sample(&mut self) -> anyhow::Result<VesselState> {
        // IMPORTANT: build the Flight in the body's *rotating* reference frame,
        // NOT `vessel.surface_reference_frame`. The surface frame is centered on
        // the vessel and moves with it, so the vessel's velocity relative to it
        // is ~0 — which makes `vertical_speed` (and horizontal speed) read zero.
        // The body's rotating frame yields true surface-relative velocities.
        let flight = self.vessel.flight(Some(&self.body_frame)).await?;

        let altitude = flight.get_surface_altitude().await?;
        let vertical_speed = flight.get_vertical_speed().await?;
        let mass = self.vessel.get_mass().await? as f64;
        let available_thrust = self.vessel.get_available_thrust().await? as f64;

        let _ = &self.sc; // kept for richer queries (staging, etc.) later.

        Ok(VesselState {
            altitude: altitude as f64,
            vertical_speed: vertical_speed as f64,
            mass,
            available_thrust,
            gravity: self.surface_gravity,
        })
    }

    async fn set_throttle(&mut self, throttle: f64) -> anyhow::Result<()> {
        let control = self.vessel.get_control().await?;
        control.set_throttle(throttle.clamp(0.0, 1.0) as f32).await?;
        Ok(())
    }
}
