import { useEffect, useRef, useState } from "react";
import type { Command, Telemetry } from "./types";

type Status = "connecting" | "open" | "closed";

/**
 * Connects to the server's telemetry WebSocket, exposing the latest telemetry
 * snapshot and a `send` function for commands. Reconnects automatically.
 */
export function useTelemetry() {
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [status, setStatus] = useState<Status>("connecting");
  const socketRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;

    const connect = () => {
      const proto = window.location.protocol === "https:" ? "wss" : "ws";
      const ws = new WebSocket(`${proto}://${window.location.host}/api/ws`);
      socketRef.current = ws;
      setStatus("connecting");

      ws.onopen = () => setStatus("open");
      ws.onmessage = (ev) => {
        try {
          setTelemetry(JSON.parse(ev.data) as Telemetry);
        } catch {
          // ignore malformed frames
        }
      };
      ws.onclose = () => {
        setStatus("closed");
        if (!closed) retry = setTimeout(connect, 1000);
      };
      ws.onerror = () => ws.close();
    };

    connect();
    return () => {
      closed = true;
      if (retry) clearTimeout(retry);
      socketRef.current?.close();
    };
  }, []);

  const send = (cmd: Command) => {
    const ws = socketRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(cmd));
    }
  };

  return { telemetry, status, send };
}
