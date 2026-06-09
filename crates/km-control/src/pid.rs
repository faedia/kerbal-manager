//! A reusable PID controller with integral anti-windup and output clamping.

use serde::{Deserialize, Serialize};

/// A standard discrete PID controller.
///
/// `update` is called once per control tick with the current `error`
/// (`setpoint - measurement`) and the elapsed time `dt`. The output is clamped
/// to `[out_min, out_max]`, and the integral term is clamped to the same range
/// to prevent windup while saturated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pid {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    /// Lower bound on the output (and, scaled by `ki`, the integral term).
    pub out_min: f64,
    /// Upper bound on the output.
    pub out_max: f64,

    integral: f64,
    prev_error: Option<f64>,
}

impl Pid {
    pub fn new(kp: f64, ki: f64, kd: f64, out_min: f64, out_max: f64) -> Self {
        Self {
            kp,
            ki,
            kd,
            out_min,
            out_max,
            integral: 0.0,
            prev_error: None,
        }
    }

    /// Clear accumulated state. Call when (re)engaging a controller so a stale
    /// integral or derivative doesn't cause a startup kick.
    pub fn reset(&mut self) {
        self.integral = 0.0;
        self.prev_error = None;
    }

    /// Advance the controller by `dt` seconds and return the clamped output.
    pub fn update(&mut self, error: f64, dt: f64) -> f64 {
        // Derivative on error. First tick has no history, so derivative = 0.
        let derivative = match (self.prev_error, dt > 0.0) {
            (Some(prev), true) => (error - prev) / dt,
            _ => 0.0,
        };
        self.prev_error = Some(error);

        // Accumulate the integral, then clamp the *term* (ki * integral) into
        // the output range so it can never wind up beyond what the actuator can
        // deliver.
        self.integral += error * dt;
        if self.ki.abs() > f64::EPSILON {
            // Bound the integral so its *term* (ki·integral) stays within the
            // output range — that's the most it could ever usefully contribute.
            let (lo, hi) = (self.out_min / self.ki, self.out_max / self.ki);
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            self.integral = self.integral.clamp(lo, hi);
        }

        let output = self.kp * error + self.ki * self.integral + self.kd * derivative;
        output.clamp(self.out_min, self.out_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_response_has_correct_sign() {
        let mut pid = Pid::new(1.0, 0.0, 0.0, -10.0, 10.0);
        assert!(pid.update(5.0, 0.1) > 0.0);
        assert!(pid.update(-5.0, 0.1) < 0.0);
    }

    #[test]
    fn output_is_clamped() {
        let mut pid = Pid::new(100.0, 0.0, 0.0, -1.0, 1.0);
        assert_eq!(pid.update(50.0, 0.1), 1.0);
        assert_eq!(pid.update(-50.0, 0.1), -1.0);
    }

    #[test]
    fn integral_does_not_wind_up_past_limits() {
        let mut pid = Pid::new(0.0, 1.0, 0.0, -1.0, 1.0);
        // Drive a large constant error for a long time.
        for _ in 0..10_000 {
            pid.update(100.0, 0.1);
        }
        // Now flip the error; the integral should recover quickly, not lag for
        // thousands of ticks because it wound up.
        let out = pid.update(-1.0, 0.1);
        assert!(out < 1.0, "integral wound up: output still saturated at {out}");
    }
}
