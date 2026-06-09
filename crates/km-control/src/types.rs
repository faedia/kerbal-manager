//! Core data types shared between controllers, the simulator, and the server.
//!
//! All quantities are SI units (meters, m/s, kilograms, newtons, m/s²).

use serde::{Deserialize, Serialize};

/// A snapshot of a vessel's flight state, as a controller sees it.
///
/// This is intentionally minimal — just enough to hover. Lateral position,
/// attitude, and orbital elements get added as the controllers grow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct VesselState {
    /// Altitude above the surface, meters.
    pub altitude: f64,
    /// Surface-relative vertical speed, m/s. Positive means climbing.
    pub vertical_speed: f64,
    /// Current total mass, kilograms.
    pub mass: f64,
    /// Thrust available at full throttle right now, newtons.
    pub available_thrust: f64,
    /// Local gravitational acceleration, m/s².
    pub gravity: f64,
}

impl VesselState {
    /// The throttle fraction needed purely to cancel gravity (hover), clamped
    /// to `[0, 1]`. Used as a feed-forward term so controllers don't have to
    /// integrate their way up from zero. Returns `0.0` if no thrust is
    /// available (e.g. flamed-out or unstaged).
    pub fn hover_throttle(&self) -> f64 {
        if self.available_thrust <= 0.0 {
            return 0.0;
        }
        let weight = self.mass * self.gravity;
        (weight / self.available_thrust).clamp(0.0, 1.0)
    }

    /// Thrust-to-weight ratio at full throttle. Below 1.0 the vessel cannot
    /// hover no matter the throttle.
    pub fn twr(&self) -> f64 {
        let weight = self.mass * self.gravity;
        if weight <= 0.0 {
            return 0.0;
        }
        self.available_thrust / weight
    }
}

/// The command a controller produces for a single tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ControlOutput {
    /// Throttle command, clamped to `[0.0, 1.0]`.
    pub throttle: f64,
}
