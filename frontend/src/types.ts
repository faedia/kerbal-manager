// Mirrors the server's `Telemetry` type (km-server/src/state.rs).
// Keep these in sync with the Rust definitions.

export interface VesselState {
  altitude: number;
  vertical_speed: number;
  mass: number;
  available_thrust: number;
  gravity: number;
}

export interface Telemetry {
  armed: boolean;
  throttle: number;
  target_altitude: number;
  state: VesselState;
  t: number;
  source: string;
}
