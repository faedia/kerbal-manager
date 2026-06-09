import { useEffect, useState } from "react";
import type { Telemetry } from "./types";

// EventSource auto-reconnects internally, so there is no terminal "closed"
// state: "connecting" covers both the initial connect and reconnect windows.
type Status = "connecting" | "open";

/**
 * Subscribes to the server's SSE telemetry stream (AD-0001), exposing the
 * latest snapshot. Reconnection is handled natively by EventSource.
 */
export function useTelemetry() {
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [status, setStatus] = useState<Status>("connecting");

  useEffect(() => {
    const source = new EventSource("/api/telemetry/stream");

    source.addEventListener("telemetry", (ev) => {
      try {
        setTelemetry(JSON.parse((ev as MessageEvent).data) as Telemetry);
      } catch {
        // ignore malformed events
      }
    });
    source.onopen = () => setStatus("open");
    source.onerror = () => setStatus("connecting"); // retrying internally

    return () => source.close();
  }, []);

  return { telemetry, status };
}
