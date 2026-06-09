//! Pure control-theory building blocks for the Kerbal flight controller.
//!
//! This crate has **no I/O and no knowledge of kRPC**. Everything here is
//! deterministic math over [`VesselState`], which makes it fully unit-testable
//! against the bundled [`RocketSim`] — you can develop controllers with Kerbal
//! Space Program closed.
//!
//! The server crate (`km-server`) is responsible for sampling real telemetry
//! into a [`VesselState`], feeding it to a controller, and pushing the
//! resulting [`ControlOutput`] back to the game.

pub mod hover;
pub mod pid;
pub mod sim;
pub mod types;

pub use hover::{HoverConfig, HoverController};
pub use pid::Pid;
pub use sim::RocketSim;
pub use types::{ControlOutput, VesselState};
