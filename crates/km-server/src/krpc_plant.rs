//! Live Kerbal Space Program plant, backed by the `krpc-client` crate.
//!
//! Compiled only under the `krpc` feature. It connects to the kRPC server
//! running inside KSP, samples the active vessel's surface-relative flight
//! state, and writes the throttle back.
//!
//! Telemetry uses kRPC **streams** rather than per-tick RPCs: the game pushes
//! updates to a client-side cache, and `sample()` just reads that cache. With
//! polling, each 50 Hz tick cost ~6 sequential RPC round-trips inside the
//! game's fixed-update budget; with streams it costs zero (the only per-tick
//! RPC left is the throttle write).

use std::sync::Arc;

use anyhow::Context;
use km_control::VesselState;
use krpc_client::services::space_center::{Control, SpaceCenter, Vessel};
use krpc_client::stream::Stream;
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
///
/// Everything fixed for the session is set up once in [`KrpcPlant::connect`]:
/// the telemetry streams, the `Control` object, and surface gravity. kRPC
/// object references live server-side until the client disconnects, so
/// creating objects per-tick would leak into the game process.
pub struct KrpcPlant {
    sc: SpaceCenter,
    vessel: Vessel,
    /// The vessel's control interface (throttle etc.).
    control: Control,
    /// Streamed altitude above the surface, meters.
    altitude: Stream<f64>,
    /// Streamed surface-relative vertical speed, m/s.
    vertical_speed: Stream<f64>,
    /// Streamed total vessel mass, kg.
    mass: Stream<f32>,
    /// Streamed available thrust at full throttle, N.
    available_thrust: Stream<f32>,
    /// Surface gravity of the orbited body, m/s². Constant for a given body.
    surface_gravity: f64,
}

impl KrpcPlant {
    /// Connect to KSP, grab the active vessel, and open telemetry streams.
    ///
    /// Under the `tokio` feature, `krpc-client` waits for each stream's first
    /// value inside the stream constructor, so `sample()` is valid immediately
    /// after this returns.
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

        // IMPORTANT: build the Flight in the body's *rotating* reference frame,
        // NOT `vessel.surface_reference_frame`. The surface frame is centered
        // on the vessel and moves with it, so velocities relative to it read
        // ~0. The body's rotating frame yields true surface-relative values.
        let body = vessel.get_orbit().await?.get_body().await?;
        let body_frame = body.get_reference_frame().await?;
        let surface_gravity = body.get_surface_gravity().await? as f64;
        let flight = vessel.flight(Some(&body_frame)).await?;

        // Open the telemetry streams (default rate: every game physics tick).
        // The Flight wrapper itself isn't needed afterwards — each stream
        // carries its own procedure call.
        let altitude = flight.get_surface_altitude_stream().await?;
        let vertical_speed = flight.get_vertical_speed_stream().await?;
        let mass = vessel.get_mass_stream().await?;
        let available_thrust = vessel.get_available_thrust_stream().await?;

        let control = vessel.get_control().await?;

        Ok(Self {
            sc,
            vessel,
            control,
            altitude,
            vertical_speed,
            mass,
            available_thrust,
            surface_gravity,
        })
    }
}

impl Plant for KrpcPlant {
    fn source(&self) -> &'static str {
        "krpc"
    }

    async fn sample(&mut self) -> anyhow::Result<VesselState> {
        // These read the client-side stream cache — no RPC round-trips.
        let altitude = self.altitude.get().await?;
        let vertical_speed = self.vertical_speed.get().await?;
        let mass = self.mass.get().await? as f64;
        let available_thrust = self.available_thrust.get().await? as f64;

        let _ = (&self.sc, &self.vessel); // kept for richer queries later.

        Ok(VesselState {
            altitude,
            vertical_speed,
            mass,
            available_thrust,
            gravity: self.surface_gravity,
        })
    }

    async fn set_throttle(&mut self, throttle: f64) -> anyhow::Result<()> {
        self.control
            .set_throttle(throttle.clamp(0.0, 1.0) as f32)
            .await?;
        Ok(())
    }
}
