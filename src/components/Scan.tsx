/** Step 2 — hardware inventory. */
import { useEffect } from "react";
import type { HardwareProfile } from "../api";
import { Notice, Spinner } from "./shared";

function gb(mb: number) {
  return Math.round(mb / 1024);
}

export function Scan({
  profile,
  scanning,
  error,
  onScan,
  onContinue,
}: {
  profile: HardwareProfile | null;
  scanning: boolean;
  error: string | null;
  onScan: () => void;
  onContinue: () => void;
}) {
  // Kick off automatically on first arrival so the flow does not stall on a
  // button the user has no reason not to press.
  useEffect(() => {
    if (!profile && !scanning && !error) onScan();
  }, [profile, scanning, error, onScan]);

  if (scanning) {
    return <Spinner label="Reading hardware, drivers, services and network configuration…" />;
  }

  if (error) {
    return (
      <div className="main-inner">
        <h1>Scan failed</h1>
        <Notice kind="danger" title="Could not complete the hardware scan">
          {error}
        </Notice>
        <div className="btn-row">
          <button className="btn btn-primary" onClick={onScan}>
            Try again
          </button>
        </div>
      </div>
    );
  }

  if (!profile) {
    return (
      <div className="main-inner">
        <h1>Scan this PC</h1>
        <div className="btn-row">
          <button className="btn btn-primary" onClick={onScan}>
            Start scan
          </button>
        </div>
      </div>
    );
  }

  const gpu = profile.gpus.find((g) => !g.is_integrated) ?? profile.gpus[0];
  const display = profile.displays.find((d) => d.is_primary) ?? profile.displays[0];
  const net =
    profile.network.find((n) => n.is_default_route) ??
    profile.network.find((n) => n.is_connected);
  const systemDrive = profile.storage.find((d) => d.is_system_drive);
  const pad = profile.peripherals.controllers[0];

  return (
    <div className="main-inner">
      <h1>What Forged found</h1>
      <p className="lede">
        This is the profile Claude will plan against. Everything below was read from the machine —
        nothing is assumed.
      </p>

      {profile.memory.rated_mhz > 0 &&
        profile.memory.configured_mhz > 0 &&
        profile.memory.rated_mhz > profile.memory.configured_mhz + 100 && (
          <Notice kind="danger" title="Your RAM is running below its rated speed">
            The modules are rated for {profile.memory.rated_mhz} MT/s but are running at{" "}
            {profile.memory.configured_mhz} MT/s — the JEDEC fallback, which means XMP/EXPO is off
            in BIOS. Turning it on is worth more 1% - low FPS than every software change in this
            app combined. It is the first item on your BIOS sheet.
          </Notice>
        )}

      {display && display.max_refresh_hz > display.refresh_hz + 5 && (
        <Notice kind="warn" title="Your monitor is not running at full refresh rate">
          Currently {display.refresh_hz} Hz on a panel that supports {display.max_refresh_hz} Hz.
        </Notice>
      )}

      {net?.is_wifi && (
        <Notice kind="warn" title="You are playing over Wi-Fi">
          Wi-Fi adds 3–15 ms of jitter that no software setting removes. An ethernet cable is the
          single biggest ping improvement available to you. Forged will still tune the wireless
          adapter as far as it can.
        </Notice>
      )}

      {!profile.fortnite.found && (
        <Notice kind="info" title="Fortnite install not found">
          Forged looked in the usual Epic Games locations. Game-specific tweaks will be skipped;
          everything else still applies.
        </Notice>
      )}

      <h2>System</h2>
      <div className="spec-grid">
        <div className="spec">
          <div className="spec-label">Processor</div>
          <div className="spec-value">{profile.cpu.name || "Unknown"}</div>
          <div className="spec-sub">
            {profile.cpu.physical_cores}C / {profile.cpu.logical_threads}T
            {profile.cpu.hybrid_architecture && " · hybrid P/E cores"}
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">Graphics</div>
          <div className="spec-value">{gpu?.name ?? "Unknown"}</div>
          <div className="spec-sub">
            {gpu?.driver_version ? `Driver ${gpu.driver_version}` : "Driver unknown"}
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">Memory</div>
          <div className="spec-value">
            {gb(profile.memory.total_mb)} GB @ {profile.memory.configured_mhz} MT/s
          </div>
          <div className="spec-sub">
            {profile.memory.module_count} module
            {profile.memory.module_count === 1 ? " — single channel" : "s"}
            {profile.memory.rated_mhz > 0 && ` · rated ${profile.memory.rated_mhz}`}
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">Display</div>
          <div className="spec-value">
            {display ? `${display.width}×${display.height} @ ${display.refresh_hz} Hz` : "Unknown"}
          </div>
          <div className="spec-sub">
            {display && display.max_refresh_hz > 0 && `Panel maximum ${display.max_refresh_hz} Hz`}
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">Network</div>
          <div className="spec-value">{net?.description ?? "Not connected"}</div>
          <div className="spec-sub">
            {net?.is_wifi ? "Wireless" : "Wired"}
            {net?.link_speed_mbps ? ` · ${net.link_speed_mbps} Mb/s link` : ""}
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">System drive</div>
          <div className="spec-value">{systemDrive?.model ?? "Unknown"}</div>
          <div className="spec-sub">
            {systemDrive?.media_type} · {systemDrive?.size_gb} GB
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">Windows</div>
          <div className="spec-value">
            {profile.os.is_windows_11 ? "Windows 11" : profile.os.caption || "Windows"}{" "}
            {profile.os.display_version}
          </div>
          <div className="spec-sub">
            Build {profile.os.build}
            {profile.os.vbs_enabled && " · VBS active"}
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">Motherboard</div>
          <div className="spec-value">
            {profile.motherboard.manufacturer} {profile.motherboard.product}
          </div>
          <div className="spec-sub">BIOS {profile.motherboard.bios_version}</div>
        </div>
      </div>

      <h2>Input devices</h2>
      <div className="spec-grid">
        <div className="spec">
          <div className="spec-label">Controller</div>
          <div className="spec-value">{pad ? pad.name : "None detected"}</div>
          <div className="spec-sub">
            {pad
              ? `${pad.kind} · ${pad.connection === "WiredUsb" ? "wired" : pad.connection === "Bluetooth" ? "Bluetooth — adds 8-12 ms over wired" : "wireless"}`
              : "Controller tweaks will be skipped"}
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">Mouse</div>
          <div className="spec-value">
            {profile.peripherals.mice[0]?.name ?? "None detected"}
          </div>
          <div className="spec-sub">
            {profile.peripherals.pointer_precision_enabled
              ? "Mouse acceleration is ON — Forged will disable it"
              : "Acceleration already off"}
          </div>
        </div>

        <div className="spec">
          <div className="spec-label">Keyboard</div>
          <div className="spec-value">
            {profile.peripherals.keyboards[0]?.name ?? "None detected"}
          </div>
          <div className="spec-sub">
            {profile.peripherals.filter_keys_enabled
              ? "Filter Keys is ON — this delays keystrokes"
              : "Accessibility filters off"}
          </div>
        </div>
      </div>

      <div className="btn-row">
        <button className="btn btn-primary" onClick={onContinue}>
          Build the optimisation plan
        </button>
        <button className="btn btn-secondary" onClick={onScan}>
          Re-scan
        </button>
      </div>
    </div>
  );
}
