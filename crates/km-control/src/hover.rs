//! A cascaded hover / altitude-hold controller.
//!
//! Structure (outer → inner):
//!
//! ```text
//!   altitude error ──(P gain)──► target vertical speed (clamped)
//!                                        │
//!   vertical-speed error ──(PID)──► throttle correction
//!                                        │
//!            hover_throttle feed-forward + correction ──► throttle [0,1]
//! ```
//!
//! The gravity feed-forward ([`VesselState::hover_throttle`]) means the PID
//! only has to trim around the hover point, so it stays well-behaved as mass
//! drops during the burn.

use serde::{Deserialize, Serialize};

use crate::pid::Pid;
use crate::types::{ControlOutput, VesselState};

/// Tunable gains for [`HoverController`]. [`Default`] is a reasonable starting
/// point for a Kerbin-launch TWR ~2 vessel; expect to tune per craft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoverConfig {
    /// Outer loop: meters of altitude error → m/s of commanded vertical speed.
    pub altitude_kp: f64,
    /// Magnitude clamp on the commanded vertical speed, m/s.
    pub max_climb_rate: f64,
    /// Inner-loop PID gains: vertical-speed error → throttle correction.
    pub vspeed_kp: f64,
    pub vspeed_ki: f64,
    pub vspeed_kd: f64,
}

impl Default for HoverConfig {
    fn default() -> Self {
        Self {
            altitude_kp: 0.5,
            max_climb_rate: 20.0,
            vspeed_kp: 0.10,
            vspeed_ki: 0.02,
            vspeed_kd: 0.0,
        }
    }
}

/// Cascaded altitude-hold controller. Drive it once per tick with
/// [`HoverController::update`].
#[derive(Debug, Clone)]
pub struct HoverController {
    pub config: HoverConfig,
    /// Desired altitude above the surface, meters.
    pub target_altitude: f64,
    vspeed_pid: Pid,
}

impl HoverController {
    pub fn new(config: HoverConfig, target_altitude: f64) -> Self {
        // The inner PID outputs a throttle *correction* in [-1, 1] that rides
        // on top of the gravity feed-forward.
        let vspeed_pid = Pid::new(
            config.vspeed_kp,
            config.vspeed_ki,
            config.vspeed_kd,
            -1.0,
            1.0,
        );
        Self {
            config,
            target_altitude,
            vspeed_pid,
        }
    }

    /// Change the altitude setpoint. Cheap; safe to call every tick.
    pub fn set_target_altitude(&mut self, altitude: f64) {
        self.target_altitude = altitude;
    }

    /// Clear the inner integrator/derivative. Call when (re)engaging.
    pub fn reset(&mut self) {
        self.vspeed_pid.reset();
    }

    /// Compute the throttle command for `state` over the last `dt` seconds.
    pub fn update(&mut self, state: &VesselState, dt: f64) -> ControlOutput {
        // Outer loop: proportional altitude hold → a target vertical speed,
        // capped so we don't command an aggressive climb from a big error.
        let altitude_error = self.target_altitude - state.altitude;
        let target_vspeed = (self.config.altitude_kp * altitude_error)
            .clamp(-self.config.max_climb_rate, self.config.max_climb_rate);

        // Inner loop: trim throttle to track the commanded vertical speed.
        let vspeed_error = target_vspeed - state.vertical_speed;
        let correction = self.vspeed_pid.update(vspeed_error, dt);

        let throttle = (state.hover_throttle() + correction).clamp(0.0, 1.0);
        ControlOutput { throttle }
    }
}
