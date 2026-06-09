//! A minimal 1-DOF (vertical) point-mass rocket simulator.
//!
//! This exists so controllers can be developed and unit-tested without Kerbal
//! Space Program running. It is deliberately simple — no aerodynamics, no
//! staging, no attitude — but it does burn propellant proportional to thrust,
//! so mass drops over the burn. That mass change is exactly the disturbance a
//! hover controller has to reject, which makes it a meaningful test bed.

use crate::types::VesselState;

/// A vertical point-mass rocket. SI units throughout.
#[derive(Debug, Clone)]
pub struct RocketSim {
    /// Altitude above the ground, meters.
    pub altitude: f64,
    /// Vertical speed, m/s.
    pub vertical_speed: f64,
    /// Current total mass, kilograms.
    pub mass: f64,
    /// Dry mass (propellant exhausted), kilograms.
    pub dry_mass: f64,
    /// Thrust at full throttle, newtons.
    pub max_thrust: f64,
    /// Gravitational acceleration, m/s² (Kerbin surface ≈ 9.81).
    pub gravity: f64,
    /// Effective exhaust velocity (Isp · g₀), m/s. Sets the fuel burn rate.
    pub exhaust_velocity: f64,
}

impl Default for RocketSim {
    /// A generic small launcher: ~2 g₀ TWR at liftoff, plenty of fuel to hover.
    fn default() -> Self {
        Self {
            altitude: 0.0,
            vertical_speed: 0.0,
            mass: 10_000.0,
            dry_mass: 4_000.0,
            max_thrust: 200_000.0,
            gravity: 9.81,
            exhaust_velocity: 2_500.0,
        }
    }
}

impl RocketSim {
    /// The flight state as a controller would observe it.
    pub fn state(&self) -> VesselState {
        VesselState {
            altitude: self.altitude,
            vertical_speed: self.vertical_speed,
            mass: self.mass,
            // Thrust is unavailable once the tanks are dry.
            available_thrust: if self.has_fuel() { self.max_thrust } else { 0.0 },
            gravity: self.gravity,
        }
    }

    /// Whether any propellant remains.
    pub fn has_fuel(&self) -> bool {
        self.mass > self.dry_mass
    }

    /// Advance the simulation by `dt` seconds at the given `throttle` (`[0,1]`).
    ///
    /// Uses semi-implicit (symplectic) Euler and a hard ground constraint so
    /// the vessel can sit on the pad without falling through it.
    pub fn step(&mut self, throttle: f64, dt: f64) {
        let throttle = throttle.clamp(0.0, 1.0);
        let thrust = if self.has_fuel() {
            self.max_thrust * throttle
        } else {
            0.0
        };

        // Rocket equation: dm/dt = -thrust / exhaust_velocity.
        if thrust > 0.0 {
            let burned = thrust / self.exhaust_velocity * dt;
            self.mass = (self.mass - burned).max(self.dry_mass);
        }

        let accel = thrust / self.mass - self.gravity;
        self.vertical_speed += accel * dt;
        self.altitude += self.vertical_speed * dt;

        // Ground constraint.
        if self.altitude <= 0.0 {
            self.altitude = 0.0;
            if self.vertical_speed < 0.0 {
                self.vertical_speed = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hover::{HoverConfig, HoverController};

    #[test]
    fn full_throttle_lifts_off() {
        let mut sim = RocketSim::default();
        for _ in 0..50 {
            sim.step(1.0, 0.02);
        }
        assert!(sim.altitude > 0.0, "should have left the pad");
        assert!(sim.vertical_speed > 0.0, "should be climbing");
    }

    #[test]
    fn hover_controller_reaches_and_holds_target_altitude() {
        let mut sim = RocketSim::default();
        let mut ctrl = HoverController::new(HoverConfig::default(), 100.0);

        let dt = 0.02; // 50 Hz control loop.
        for _ in 0..(60.0 / dt) as usize {
            let out = ctrl.update(&sim.state(), dt);
            sim.step(out.throttle, dt);
        }

        // After 60 s of settling it should hold ~100 m with near-zero vspeed.
        assert!(
            (sim.altitude - 100.0).abs() < 5.0,
            "altitude settled at {:.2} m (want ~100)",
            sim.altitude
        );
        assert!(
            sim.vertical_speed.abs() < 2.0,
            "vertical speed was {:.2} m/s (want ~0)",
            sim.vertical_speed
        );
    }
}
