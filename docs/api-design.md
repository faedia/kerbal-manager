# API Design: REST commands + streamed telemetry

**Status:** proposed
**Date:** 2026-06-09
**Scope:** the HTTP surface between `frontend/` and `km-server` (`crates/km-server/src/api.rs`)

## Summary

Split the API by traffic type:

- **Actions** (arm, disarm, set target altitude) become **plain REST endpoints**.
  Each command gets a URL, validation, a status code, and an error body.
- **Telemetry** (altitude, velocity, throttle, …) is **streamed server → client
  over Server-Sent Events (SSE)**, replacing the WebSocket.

The WebSocket endpoint is removed. Nothing in the current system needs a
bidirectional socket once commands are REST, and SSE is strictly simpler for a
one-way stream. A WebSocket may return later for one specific future need
(continuous manual control input); see [Future work](#future-work).

## Current state

`api.rs` exposes three endpoints today:

| Endpoint | Role |
|---|---|
| `GET /api/telemetry` | latest snapshot (JSON) |
| `POST /api/command` | tagged-enum `Command` body, fire-and-forget |
| `GET /api/ws` | WebSocket: pushes telemetry **and** accepts commands |

The frontend uses the WebSocket for everything: `useTelemetry.ts` parses
inbound frames as telemetry and serializes commands onto the same socket.

## Why split the API

The two kinds of traffic have opposite characteristics:

| | Telemetry | Commands |
|---|---|---|
| Direction | server → client only | client → server |
| Rate | up to 50 Hz (control loop rate) | human-initiated, < 1 Hz |
| Semantics | latest value wins; stale data is worthless | each one matters; needs an answer |
| On reconnect | just want the newest snapshot | must know whether it was received |
| Natural shape | a stream | request/response |

Forcing both through one WebSocket means inventing request/response on top of
a stream — correlation IDs, an ack message type, client-side timeout logic —
which is exactly the machinery HTTP already provides. The current code shows
the cost of *not* building that machinery:

1. **Commands have no feedback.** The WS handler forwards commands with
   `let _ = state.commands.send(cmd).await;` and logs parse errors
   server-side only. A malformed or dropped command is invisible to the
   client. With REST, the same failure is a `422` or `503` with a body.
2. **Commands silently vanish during reconnects.** The frontend's `send()`
   no-ops when the socket isn't `OPEN`. Clicking **ARM** during the 1-second
   reconnect window does nothing, with no error surfaced. A `fetch()` either
   completes or rejects — the UI can react either way.
3. **No validation surface.** There is no natural place on a WS frame to say
   "altitude must be finite and ≥ 0". A REST handler validates at the edge
   and returns a structured error before anything reaches the control loop.
4. **Tooling.** REST commands work from `curl`, scripts, and future mission
   automation without a WS client library or knowledge of the frame protocol:
   `curl -X POST localhost:8080/api/vessel/arm`.
5. **Later cross-cutting concerns** (auth, rate limiting, audit logging) are
   per-request middleware on REST; on a WS they must be reimplemented
   per-message inside the socket handler.

**Latency is not a counterargument here.** The control loop ticks at 50 Hz
(20 ms), and it drains the command queue at the top of each tick. An HTTP
POST on localhost costs ~1 ms — the loop's own tick boundary dominates,
identically for both transports.

**When commands-over-socket *would* win:** high-rate continuous input, e.g.
a manual-fly mode streaming joystick/throttle axes at 20–60 Hz. Per-request
HTTP overhead and head-of-line blocking would matter there. That mode does
not exist yet; when it does, it should get its own dedicated input channel
rather than dragging discrete commands back onto a socket (see
[Future work](#future-work)).

### Verdict

The split is the right call. The current bugs (1) and (2) are not incidental
— they are the predictable result of using a stream for request/response
traffic and would require building an ack protocol to fix in place.

## Telemetry transport: SSE over WebSocket

Once commands move to REST, the stream is strictly one-directional, which
removes the only reason to use a WebSocket. SSE (`text/event-stream`) fits
better:

| | SSE | WebSocket |
|---|---|---|
| Direction | server → client (exactly what we need) | bidirectional (unused) |
| Client API | `EventSource` — ~5 lines, **auto-reconnect built in** | manual reconnect/backoff code (currently ~25 lines of `useTelemetry.ts`) |
| Protocol | plain HTTP response | upgrade handshake, frame protocol |
| Debugging | `curl -N localhost:8080/api/telemetry/stream` | needs a WS client |
| Proxies / middleware | ordinary HTTP — CORS, auth, compression all apply | special-cased everywhere |
| Lost messages on reconnect | irrelevant — telemetry is snapshot-based | same |

The `watch`-channel semantics make SSE's main weakness (no replay of missed
events without extra work) a non-issue: every event is a complete snapshot,
so a reconnecting client is fully caught up by the first event it receives.
This is the same property the current WS handler relies on when it sends the
snapshot immediately on connect.

Binary framing is the one thing WS offers that SSE can't, but telemetry is a
~200-byte JSON object; at 50 Hz that is ~10 KB/s. Not worth optimizing.

## API specification

All endpoints live under `/api`. No version prefix for now — this is a
single-binary app where the server ships its own frontend, so the two sides
upgrade atomically; versioning is ceremony until there are external clients.

### Conventions

- Request and response bodies are JSON (`Content-Type: application/json`),
  except the SSE stream.
- Field names are `snake_case`, mirroring the Rust serde derives.
- Errors use one shape everywhere:

```json
{ "error": "altitude must be a finite number >= 0" }
```

| Status | Meaning |
|---|---|
| `200 OK` | read succeeded |
| `202 Accepted` | command validated and queued for the control loop |
| `404 Not Found` | unknown route |
| `422 Unprocessable Entity` | body failed validation |
| `503 Service Unavailable` | control loop is down (command channel closed) |

### Commands (REST)

One endpoint per action, replacing the tagged-enum `POST /api/command`.
Distinct routes are self-documenting, give each action its own validation,
and let the HTTP method carry meaning (`PUT` for the idempotent setpoint).

Routes are rooted at `/api/vessel/…` — singular, because the server flies
one vessel today. The multi-vessel roadmap item maps cleanly onto
`/api/vessels/{id}/…` later without renaming verbs.

#### `POST /api/vessel/arm`

Engage the hover controller (resets controller integrators first).
No body. Idempotent in effect: arming while armed re-arms (resets
integrators), matching current control-loop behavior.

- `202 Accepted` — queued. Body: `{ "queued": true }`
- `503` — control loop unavailable.

#### `POST /api/vessel/disarm`

Cut throttle and disengage. No body.

- `202 Accepted` / `503` as above.

#### `PUT /api/vessel/target-altitude`

Set the altitude setpoint (meters above surface).

```json
{ "altitude": 150.0 }
```

Validation: must be present, finite, and `>= 0`; otherwise `422`.

- `202 Accepted` / `422` / `503`.

#### Command acknowledgment semantics

`202 Accepted` means *validated and queued*, not *applied*. This is
deliberate. In a control system, **command and state are different things**:
the source of truth for "is the vessel armed" is the telemetry stream, not
the command response. The control loop applies queued commands at the top of
its next tick (≤ 20 ms), so the UI sees the effect in the very next
telemetry event — faster than human perception. The UI should render state
from telemetry (`armed`, `target_altitude`) and never optimistically toggle
from a command response.

This avoids a real complication: a synchronous "applied" ack would require
threading a oneshot reply channel through the `Command` enum and the control
loop, and would still race against the stream (the ack and the telemetry
event travel on different connections, so ordering between them is undefined
either way). Convergence-via-telemetry is simpler and honest. If a future
command needs a result that telemetry can't express, add a reply channel for
that command then.

### Telemetry

#### `GET /api/telemetry` (unchanged)

Latest snapshot as JSON. Useful for scripts and polling clients.

#### `GET /api/telemetry/stream` (new, replaces `GET /api/ws`)

SSE stream (`Content-Type: text/event-stream`). On connect, the current
snapshot is sent immediately, then one event per control-loop publish that
the client keeps up with (the `watch` channel coalesces under backpressure —
slow clients skip intermediate snapshots and always get the newest, never a
growing queue).

```
event: telemetry
data: {"armed":true,"throttle":0.62,"target_altitude":100.0,"state":{...},"t":12.3,"source":"sim"}
```

Events are named (`event: telemetry`) so future event types — alerts, mode
transitions, vessel-list changes — can share the stream without breaking
existing listeners.

Optional, can be deferred: `?hz=10` to decimate server-side for clients that
don't want 50 events/sec. The UI is fine at full rate; this matters only if
telemetry grows (e.g. full 3-DOF state) or clients multiply.

The telemetry JSON schema is unchanged from today
([state.rs](../crates/km-server/src/state.rs) `Telemetry` /
[types.ts](../frontend/src/types.ts)).

### Command/state flow

```
UI                          km-server                    control loop (50 Hz)
│                               │                               │
│ PUT /api/vessel/target-altitude                               │
│──────────────────────────────►│ validate                      │
│                               │ mpsc.send(SetTargetAltitude)──►│ (queued)
│◄──────────────────────────────│ 202 {"queued":true}           │
│                               │                          tick: drain queue,
│                               │                          apply, publish
│   SSE: event telemetry        │◄───── watch channel ──────────│
│◄──────────────────────────────│  {"target_altitude":150,...}  │
│  UI re-renders from telemetry │                               │
```

## Implementation notes

### Server (`api.rs`)

- Router becomes:
  ```text
  GET  /api/telemetry              (keep)
  GET  /api/telemetry/stream       (new: SSE)
  POST /api/vessel/arm             (new)
  POST /api/vessel/disarm          (new)
  PUT  /api/vessel/target-altitude (new)
  ```
  `POST /api/command` and `GET /api/ws` are deleted (see migration).
- SSE in axum: `axum::response::sse::{Sse, Event, KeepAlive}` over a stream
  built from `tokio_stream::wrappers::WatchStream` on the existing
  `watch::Receiver<Telemetry>` — no new state plumbing. At 50 Hz the data is
  its own heartbeat, but add `KeepAlive` anyway so an idle/paused loop
  doesn't let proxies time the connection out.
- Command handlers share a small helper: validate → `state.commands.send()`
  → map `Ok` to `202`, channel-closed to `503`.

### Frontend

- `useTelemetry.ts` shrinks: `new EventSource("/api/telemetry/stream")`,
  `onmessage`/`addEventListener("telemetry", …)` → `setTelemetry`. Delete
  the manual reconnect timer (EventSource retries natively); keep the
  `status` value driven by `onopen`/`onerror`.
- New `api.ts` with typed command helpers:
  ```ts
  export const arm  = () => post("/api/vessel/arm");
  export const disarm = () => post("/api/vessel/disarm");
  export const setTargetAltitude = (altitude: number) =>
    put("/api/vessel/target-altitude", { altitude });
  ```
  Each returns a promise that rejects on non-2xx, so the UI can surface
  failures (e.g. disable buttons / show a toast) instead of today's silent
  drop when the socket is closed.
- `App.tsx` keeps rendering exclusively from telemetry; only the `send`
  call sites change.
- Vite dev proxy: SSE works through the existing `/api` proxy; ensure the
  proxy config doesn't buffer (`changeOrigin` + default settings are fine
  for `http-proxy`; no WS-specific proxy config needed anymore).

### Migration

The server ships its own frontend, so there are no third-party clients to
break. Do it in one change set:

1. Add the new endpoints (SSE + command routes) alongside the old ones.
2. Switch the frontend to them.
3. Delete `GET /api/ws` and `POST /api/command` in the same PR. Keeping dead
   transports "for compatibility" in a pre-1.0 single-client project is pure
   maintenance cost.

## Alternatives considered

- **Everything over WebSocket (status quo, hardened).** Add correlation IDs,
  ack frames, and client timeout/retry to fix the feedback problem. Rebuilds
  HTTP inside a socket; more code on both sides; still opaque to curl and
  middleware. Rejected.
- **Everything over REST (polling).** `GET /api/telemetry` on an interval.
  Simple, but at UI-acceptable rates (≥ 5 Hz) it's strictly worse than SSE
  in latency, overhead, and battery, with no upside. Rejected.
- **Keep WS for telemetry, REST for commands.** Workable — the split is the
  important part — but keeps the upgrade handshake, frame handling, and
  hand-rolled reconnect for a now one-directional stream that SSE handles
  natively. Choose WS here only if binary framing or client→server streaming
  becomes necessary first. Rejected for now.

## Future work

- **Manual-fly mode** (continuous stick/throttle input) is the one feature
  on the horizon that justifies a client→server stream. Give it a dedicated
  `GET /api/vessel/input` WebSocket carrying *only* axis frames, with
  last-value-wins semantics mirroring the telemetry `watch` channel.
  Discrete commands stay on REST regardless.
- **Multi-vessel:** commands move to `/api/vessels/{id}/…`; the SSE stream
  either gains per-vessel event names or a `?vessel=` filter.
- **Auth:** when the server is exposed beyond localhost, both REST and SSE
  accept standard header/cookie middleware unchanged — one of the reasons
  for this design.
