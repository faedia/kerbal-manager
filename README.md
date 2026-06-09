# Kerbal Manager

A flight controller and automation system for [Kerbal Space Program](https://www.kerbalspaceprogram.com/),
driving vehicles remotely through the [kRPC](https://krpc.github.io/krpc/) mod.

Control theory and the backend are written in **Rust**; the operator UI is a
**React + TypeScript** web app. The current milestone is a **hover / altitude-hold**
controller. Orbit insertion, rendezvous, and docking are on the roadmap.

> [!NOTE]
> I use this project to **experiment with AI-agent development processes**.
> Much of the code, the reviews, the design records (see [docs/](docs/)), and
> the commit history is produced by AI agents working under my direction —
> how well that workflow holds up is as much the point of this repo as the
> rockets are.

## Architecture

```
┌─────────────┐      REST + SSE            ┌──────────────────────────┐
│  React UI   │ ◄────────────────────────► │        km-server         │
│ (frontend/) │  cmds in (REST), telemetry │  web API + control loop  │
└─────────────┘   out (SSE stream)         └───────────┬──────────────┘
                                                        │ Plant trait
                                        ┌───────────────┴───────────────┐
                                        │                               │
                                  ┌───────────┐                 ┌──────────────┐
                                  │ SimPlant  │   (default)     │  KrpcPlant   │  (feature "krpc")
                                  │ offline   │                 │  live KSP    │
                                  │ rocket sim│                 │ via kRPC TCP │
                                  └───────────┘                 └──────────────┘
                                        ▲
                                        │ VesselState / ControlOutput
                                  ┌───────────┐
                                  │ km-control│  pure control theory (PID, hover), no I/O
                                  └───────────┘
```

| Crate / dir   | Responsibility |
|---------------|----------------|
| `crates/km-control` | Pure, deterministic control theory: `Pid`, cascaded `HoverController`, shared `VesselState`/`ControlOutput` types, and a 1-DOF `RocketSim`. No I/O — fully unit-testable. |
| `crates/km-server`  | The real-time control loop (50 Hz, generic over a `Plant`), the kRPC link, and an [axum](https://docs.rs/axum) HTTP server (REST commands + SSE telemetry, see [AD-0001](docs/api-design.md)) that also serves the built frontend. |
| `frontend`          | React + TypeScript dashboard: live telemetry over SSE, arm/disarm, set target altitude. |

**Key design choice:** controllers never touch kRPC. They consume a `VesselState`
and emit a `ControlOutput`, so the *exact same* loop runs against the offline
simulator or a live vessel. You can develop and test control logic with KSP closed.

## Quick start

### Backend (offline simulator — no KSP needed)

```sh
cargo run -p km-server
# serves http://localhost:8080  (telemetry + API + built frontend)
```

### Frontend (dev server with hot reload)

```sh
cd frontend
npm install      # first time only
npm run dev      # http://localhost:5173, proxies /api to the Rust server
```

Open http://localhost:5173, click **ARM HOVER**, and watch the simulated rocket
climb to the target altitude and hold. For a production bundle, `npm run build`
writes `frontend/dist`, which `km-server` serves directly at `:8080`.

### Flying the real game

1. Install the kRPC mod in KSP, launch a vessel, and start the kRPC server
   (default `127.0.0.1:50000/50001`).
2. Run with the live link enabled — `KM_KRPC` must be set **before** launch:

   ```powershell
   # PowerShell
   $env:KM_KRPC = "1"; cargo run -p km-server --features krpc
   ```

   ```sh
   # bash
   KM_KRPC=1 cargo run -p km-server --features krpc
   ```

   The kRPC link lives entirely in `crates/km-server/src/krpc_plant.rs` and is
   compiled out by default, so the rest of the stack always builds without it.

> `krpc_plant.rs` compiles against the real `krpc-client` API (method names are
> type-checked). Telemetry uses kRPC **streams** — the game pushes updates to a
> client-side cache, so each control tick costs zero telemetry RPCs (only the
> throttle write goes over the wire). Velocities come from the body's **rotating**
> reference frame — using `vessel.surface_reference_frame` instead makes
> `vertical_speed` read zero, since that frame moves with the vessel.

## Configuration (env vars)

| Var | Default | Meaning |
|-----|---------|---------|
| `KM_BIND` | `127.0.0.1:8080` | Server bind address. The API is unauthenticated and can arm the vessel, so it binds loopback by default; set `0.0.0.0:8080` to expose it on the LAN deliberately. |
| `KM_FRONTEND_DIST` | `frontend/dist` | Directory of built frontend assets. |
| `KM_KRPC` | _(unset)_ | If truthy (and built with `--features krpc`), connect to live KSP. `0`/`false`/`no`/`off`/empty count as unset. |
| `RUST_LOG` | `info,km_server=debug` | Tracing filter. |

## Tests

```sh
cargo test
```

Includes a closed-loop test proving the hover controller drives the simulated
rocket to its target altitude and holds it as mass drops during the burn.

## Roadmap

- [x] Cascaded hover / altitude-hold controller + offline sim
- [x] Web dashboard with live telemetry and basic commands
- [ ] Validate the kRPC link against a live vessel
- [ ] Attitude control (replace SAS-held orientation with our own controller)
- [ ] Lateral translation / position hold (full 3-DOF hover)
- [ ] Ascent guidance and orbit insertion
- [ ] Multi-vessel view; rendezvous and docking
