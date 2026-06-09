import { useState } from "react";
import { useTelemetry } from "./useTelemetry";

export default function App() {
  const { telemetry, status, send } = useTelemetry();
  const [targetInput, setTargetInput] = useState("100");

  const t = telemetry;
  const twr =
    t && t.state.mass * t.state.gravity > 0
      ? t.state.available_thrust / (t.state.mass * t.state.gravity)
      : 0;

  return (
    <div style={styles.page}>
      <header style={styles.header}>
        <h1 style={styles.h1}>Kerbal Manager</h1>
        <span style={styles.badge(status === "open")}>
          {status === "open" ? `live · ${t?.source ?? "?"}` : status}
        </span>
      </header>

      <section style={styles.grid}>
        <Metric label="Altitude" value={fmt(t?.state.altitude)} unit="m" />
        <Metric label="Vertical speed" value={fmt(t?.state.vertical_speed)} unit="m/s" />
        <Metric label="Target altitude" value={fmt(t?.target_altitude)} unit="m" />
        <Metric label="Throttle" value={t ? (t.throttle * 100).toFixed(0) : "—"} unit="%" />
        <Metric label="Mass" value={fmt(t?.state.mass, 0)} unit="kg" />
        <Metric label="TWR" value={twr ? twr.toFixed(2) : "—"} unit="" />
      </section>

      <section style={styles.controls}>
        <button
          style={styles.button(t?.armed ? "#b23" : "#2a7")}
          onClick={() => send({ type: t?.armed ? "disarm" : "arm" })}
        >
          {t?.armed ? "DISARM" : "ARM HOVER"}
        </button>

        <div style={styles.targetRow}>
          <input
            style={styles.input}
            type="number"
            value={targetInput}
            onChange={(e) => setTargetInput(e.target.value)}
          />
          <button
            style={styles.button("#357")}
            onClick={() => {
              const altitude = Number(targetInput);
              if (!Number.isNaN(altitude)) {
                send({ type: "set_target_altitude", altitude });
              }
            }}
          >
            Set target altitude (m)
          </button>
        </div>
      </section>

      <p style={styles.hint}>
        Running against the <strong>{t?.source ?? "…"}</strong> plant. Arm to engage the
        hover controller; it climbs to the target altitude and holds.
      </p>
    </div>
  );
}

function Metric({ label, value, unit }: { label: string; value: string; unit: string }) {
  return (
    <div style={styles.metric}>
      <div style={styles.metricLabel}>{label}</div>
      <div style={styles.metricValue}>
        {value} <span style={styles.metricUnit}>{unit}</span>
      </div>
    </div>
  );
}

function fmt(n: number | undefined, digits = 1): string {
  return n === undefined ? "—" : n.toFixed(digits);
}

const styles = {
  page: {
    fontFamily: "system-ui, sans-serif",
    background: "#0b0f14",
    color: "#e6edf3",
    minHeight: "100vh",
    margin: 0,
    padding: "2rem",
    boxSizing: "border-box" as const,
  },
  header: { display: "flex", alignItems: "center", gap: "1rem" },
  h1: { fontSize: "1.5rem", margin: 0 },
  badge: (ok: boolean) => ({
    fontSize: "0.8rem",
    padding: "0.2rem 0.6rem",
    borderRadius: "999px",
    background: ok ? "#11331f" : "#3a1111",
    color: ok ? "#5fe39a" : "#ff8080",
    border: `1px solid ${ok ? "#1e5e38" : "#5e1e1e"}`,
  }),
  grid: {
    display: "grid",
    gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))",
    gap: "1rem",
    margin: "1.5rem 0",
  },
  metric: {
    background: "#11171f",
    border: "1px solid #1d2733",
    borderRadius: "10px",
    padding: "1rem",
  },
  metricLabel: { fontSize: "0.75rem", color: "#8b98a5", textTransform: "uppercase" as const },
  metricValue: { fontSize: "1.6rem", fontWeight: 600, marginTop: "0.3rem" },
  metricUnit: { fontSize: "0.9rem", color: "#8b98a5", fontWeight: 400 },
  controls: { display: "flex", flexWrap: "wrap" as const, gap: "1rem", alignItems: "center" },
  targetRow: { display: "flex", gap: "0.5rem", alignItems: "center" },
  input: {
    background: "#0b0f14",
    color: "#e6edf3",
    border: "1px solid #2a3744",
    borderRadius: "8px",
    padding: "0.6rem",
    width: "100px",
    fontSize: "1rem",
  },
  button: (bg: string) => ({
    background: bg,
    color: "white",
    border: "none",
    borderRadius: "8px",
    padding: "0.7rem 1.1rem",
    fontSize: "1rem",
    fontWeight: 600,
    cursor: "pointer",
  }),
  hint: { color: "#8b98a5", marginTop: "2rem", fontSize: "0.9rem" },
};
