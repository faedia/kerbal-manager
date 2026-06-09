//! The [`Plant`] abstraction: the thing the control loop reads from and writes
//! to. Decoupling this from the controller lets the exact same loop run against
//! the offline [`SimPlant`] or, with the `krpc` feature, a live KSP vessel.

use km_control::VesselState;

/// A controllable vehicle the loop can sample and actuate.
///
/// Implementors own their own timing model: [`SimPlant`] advances an internal
/// simulation on each `sample`, while a kRPC plant just reads the live game.
pub trait Plant: Send {
    /// A short tag identifying the plant for telemetry (`"sim"` / `"krpc"`).
    fn source(&self) -> &'static str;

    /// Read the current flight state.
    fn sample(&mut self) -> impl std::future::Future<Output = anyhow::Result<VesselState>> + Send;

    /// Command the main-engine throttle, `[0, 1]`.
    fn set_throttle(
        &mut self,
        throttle: f64,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

/// A [`Plant`] backed by the in-process [`km_control::RocketSim`].
///
/// Each `sample` advances the simulation by one control tick using the most
/// recently commanded throttle, so the loop sees realistic, throttle-dependent
/// dynamics without KSP.
pub struct SimPlant {
    sim: km_control::RocketSim,
    dt: f64,
    throttle: f64,
}

impl SimPlant {
    /// `dt` should match the control loop's tick period (seconds).
    pub fn new(dt: f64) -> Self {
        Self {
            sim: km_control::RocketSim::default(),
            dt,
            throttle: 0.0,
        }
    }
}

impl Plant for SimPlant {
    fn source(&self) -> &'static str {
        "sim"
    }

    async fn sample(&mut self) -> anyhow::Result<VesselState> {
        // Advance using the throttle commanded on the previous tick.
        self.sim.step(self.throttle, self.dt);
        Ok(self.sim.state())
    }

    async fn set_throttle(&mut self, throttle: f64) -> anyhow::Result<()> {
        self.throttle = throttle.clamp(0.0, 1.0);
        Ok(())
    }
}
