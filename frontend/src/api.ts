// Typed command helpers for the REST API (AD-0001).
//
// Each returns a promise that resolves when the command is validated and
// queued (202) and rejects with the server's error message otherwise — so the
// UI can surface failures instead of silently dropping commands. Note that
// 202 means *queued*, not *applied*: state truth (armed, target_altitude)
// comes from the telemetry stream, which reflects the command on the next
// control-loop tick.

async function send(method: string, path: string, body?: unknown): Promise<void> {
  const res = await fetch(path, {
    method,
    headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const data = (await res.json()) as { error?: string };
      if (data.error) message = data.error;
    } catch {
      // non-JSON error body; keep the status line
    }
    throw new Error(message);
  }
}

/** Engage the hover controller. */
export const arm = () => send("POST", "/api/vessel/arm");

/** Disengage; the control loop cuts throttle and goes hands-off. */
export const disarm = () => send("POST", "/api/vessel/disarm");

/** Set the altitude setpoint, meters above the surface. */
export const setTargetAltitude = (altitude: number) =>
  send("PUT", "/api/vessel/target-altitude", { altitude });
