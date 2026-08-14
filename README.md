# Forged

AI-planned Windows 11 optimisation for a dedicated Fortnite machine.

Forged scans the PC, has Claude decide which of its 125 vetted optimisations apply to that
specific hardware, applies them reversibly, and produces a BIOS checklist for the changes
software cannot make.

---

## The three rules

Everything in this codebase follows from three constraints. They are worth stating up front
because they are what separate this from a batch file with a UI.

### 1. The AI selects. It never authors.

The planner is sent the machine profile and the catalog's **metadata** — IDs, names, summaries,
risk and impact ratings. It returns a list of **IDs**. It never sees a registry path and has no
mechanism for returning one, and every ID it returns is checked against the catalog before the
engine runs.

```rust
let Some(tweak) = catalog::find(&entry.id) else {
    report.unknown_ids.push(entry.id);   // discarded, never executed
    continue;
};
```

This is the difference between *"the AI is unlikely to break your PC"* and *"the AI cannot break
your PC"*. A hallucinated tweak ID is caught by a hash lookup. A hallucinated registry write would
not be catchable at all.

What the model actually does is the part it is good at: weighing 125 vetted changes against real
silicon, ordering them, and explaining each one in terms of *your* CPU, *your* RAM speed, *your*
refresh rate.

### 2. Nothing is applied that cannot be un-applied.

Registry and service writes capture their prior value at write time. Command actions must declare
an inverse or the engine refuses them. The journal is written to disk after **every** entry, so a
power cut mid-run still leaves a complete record of what changed.

```rust
UndoRecord::Registry { hive, path, value, previous: Option<RegData> }
//                                        None => the value did not exist; undo deletes it
```

A System Restore point is taken first as well, but it is the fallback, not the mechanism — the
journal is far more precise, and it works without System Restore being enabled.

### 3. Nothing touches the game.

Easy Anti-Cheat treats process interference as a ban condition, and it is right to. Forged tunes
the operating system *around* Fortnite and never reads, writes, injects into, or hooks the game
process.

The one place this needs care is process priority. Setting an Image File Execution Options
priority class tells the **Windows loader** what priority to start a process at — the same
mechanism Task Manager uses, applied before the process exists. That is categorically different
from writing to a running game's memory, and Forged only ever does the former.

---

## Honesty as a feature

Roughly 60% of the "gaming optimisation" advice in circulation does nothing. Forged ships those
entries too, tagged:

```rust
pub enum Evidence {
    Measured,           // reproducible in benchmarks
    Documented,         // follows from documented Windows behaviour
    SituationalGain,    // helps on loaded systems, not clean ones
    NoMeasuredBenefit,  // folklore. Applied because it is harmless and people look for it.
}
```

`NoMeasuredBenefit` entries are applied — they are inert, and their absence reads as an incomplete
tool — but they are **excluded from the report's headline number** and labelled in the UI. A tool
that claims 40 improvements when 12 are real is lying by arithmetic.

Several catalog entries exist purely to **undo damage other tools do**:

| Entry | What it repairs |
|---|---|
| `net.restore_tcp_autotuning` | Reverses the widespread advice to disable autotuning, which throttles throughput and does nothing for latency |
| `controller.keep_xbox_accessory_service` | Debloat scripts disable `XboxGipSvc`, which breaks wired Xbox controllers outright |
| `fortnite.easyanticheat_service` | Debloat scripts disable EasyAntiCheat, after which Fortnite will not launch |
| `cpu.remove_platform_clock_override` | Removes forced-HPET boot flags, which are slower than the invariant TSC on any modern CPU |
| `cpu.hybrid_scheduler_hint` | Protects Intel Thread Director on 12th-gen+, where guides written for older CPUs cost 20%+ |

Windows Defender is deliberately left running. The correct fix is an exclusion for the game
directory, which removes the scanning cost on the hot path without leaving the machine
unprotected.

---

## Sections

| Section | Focus |
|---|---|
| Controller | Gamepad latency, USB idle suspend, Guide-button overlay hooks |
| Keyboard & Mouse | Acceleration, pointer curves, accessibility input filters, HID power |
| Network & Ping | Nagle, interrupt moderation, offloads, multimedia throttle, EEE |
| GPU | HAGS (per-vendor), Game DVR, MPO, clock management, shader cache |
| CPU | Power plan, core parking, scheduling quantum, VBS |
| Memory | Paging, service consolidation, prefetch, compression |
| Storage | Filesystem bookkeeping, indexing, scheduled maintenance |
| System | Telemetry, background apps, shell overhead, Defender exclusion |
| Latency & DPC | MSI mode, interrupt priority, timer resolution, PCIe ASPM |
| Fortnite | Process priority, GPU pinning, launcher and shader cache |

---

## Architecture

```
crates/forged-core/     Pure Rust. No Tauri. All the real logic.
  scan.rs               CIM + registry inventory
  tweaks/catalog/       125 declarations across 10 files
  tweaks/engine.rs      The only thing that mutates the machine
  journal.rs            Undo records, atomic persistence, revert
  ai.rs                 Claude planner + the validation boundary
  bios.rs               Firmware checklist, vendor-specific menu paths
  win/                  The entire OS surface, in four files
src-tauri/              Thin command layer. Marshals and nothing else.
src/                    React + TypeScript UI
```

The core crate is deliberately platform-independent so the catalog invariants, journal
round-tripping and AI validation boundary are all unit-testable on Linux CI without a Windows
runner. Actuators refuse to run off-Windows rather than silently no-op.

Keeping the whole OS boundary in `win/` — four files — means every mutation the app is capable of
is reviewable by reading them.

---

## Building

The installer is built by CI on a Windows runner; see `.github/workflows/build.yml`. Grab
`Forged-Windows-Installer` from the run's artifacts, or from the Releases page for tagged builds.

Locally, on Windows:

```bash
npm install
npm run tauri build      # produces target/.../bundle/nsis/Forged_1.0.0_x64-setup.exe
```

Tests and the Windows cross-check run anywhere:

```bash
cargo test -p forged-core
cargo clippy -p forged-core -- -D warnings
cargo check -p forged-core --target x86_64-pc-windows-gnu   # needs mingw-w64
npm run build
```

---

## API key

Forged asks for your own Anthropic API key rather than shipping one. A key compiled into a
distributed binary is extractable with a hex editor in about thirty seconds — obfuscation only
changes how long it takes — and whoever shipped it pays for every call made with it.

The key is encrypted with DPAPI under your Windows account and stored in
`%APPDATA%\Forged\credentials.bin`. Copying that file to another machine or another user profile
yields nothing. `ANTHROPIC_API_KEY` is honoured if set.

Without a key, Forged falls back to a deterministic plan that selects every applicable catalog
entry. It works, but it cannot weigh tradeoffs, order by dependency, or explain itself in terms of
your hardware — and the report says so plainly rather than pretending otherwise.

---

## Elevation

Forged requests `requireAdministrator` in its manifest. It writes to HKLM, reconfigures services
and calls `powercfg` — none of which work from a standard-user token. It asks once at launch
rather than failing halfway through a run with half a plan applied.

The webview itself is granted `core:default` and nothing else. Every privileged operation goes
through a named Rust command.

---

## Rollback

Every run is journalled to `%ProgramData%\Forged\journals\`. The Rollback screen lists them and
restores any run's exact prior values — entries are unwound in reverse order, and a failure on one
record never stops the rest, because a partial restore beats abandoning the remainder. Anything
that could not be restored is listed explicitly.

If the machine will not boot far enough to run Forged: the restore point taken before the run, or
Safe Mode.
