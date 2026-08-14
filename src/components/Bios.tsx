/** Step 5 — the firmware checklist the user applies by hand. */
import { useState } from "react";
import type { Report } from "../api";
import { Notice } from "./shared";

export function Bios({ report }: { report: Report | null }) {
  const [copied, setCopied] = useState(false);

  if (!report || !report.bios_markdown) {
    return (
      <div className="main-inner">
        <h1>BIOS checklist</h1>
        <p className="lede">
          Apply a plan first — the checklist is generated from your motherboard and memory.
        </p>
      </div>
    );
  }

  async function copy() {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(report.bios_markdown);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard can be unavailable in the webview; the text is selectable
      // below either way, so this is not worth an error state.
    }
  }

  return (
    <div className="main-inner">
      <h1>BIOS checklist</h1>
      <p className="lede">
        These are the changes Forged cannot make for you. Firmware writes have no undo and the
        vendor tooling to do them from Windows can brick a board, so this is a checklist rather
        than a button.
      </p>

      <Notice kind="info" title="If the machine will not boot after a change">
        Clear CMOS — either the jumper on the board or by pulling the coin cell for a minute. That
        reverts every firmware change and costs nothing but the time.
      </Notice>

      <div className="btn-row" style={{ marginTop: 0, marginBottom: 18 }}>
        <button className="btn btn-secondary" onClick={copy}>
          {copied ? "Copied" : "Copy to clipboard"}
        </button>
        <span className="muted small">
          Paste it into your phone so you can read it while the PC is in BIOS.
        </span>
      </div>

      <div className="bios-md selectable">{report.bios_markdown}</div>
    </div>
  );
}
