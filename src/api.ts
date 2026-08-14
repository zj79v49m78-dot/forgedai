/**
 * Typed bridge to the Rust backend.
 *
 * Every type here mirrors a `serde` struct in `forged-core`. They are kept in
 * one file so a change on the Rust side has exactly one place to be reflected.
 */
import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Shared enums
// ---------------------------------------------------------------------------

export type Section =
  | "Controller"
  | "KeyboardMouse"
  | "Network"
  | "Gpu"
  | "Cpu"
  | "Memory"
  | "Storage"
  | "System"
  | "Latency"
  | "Fortnite";

export type Risk = "Low" | "Medium" | "High";
export type Impact = "Major" | "Moderate" | "Minor";
export type Evidence =
  | "Measured"
  | "Documented"
  | "SituationalGain"
  | "NoMeasuredBenefit";
export type Severity = "Critical" | "High" | "Medium" | "Info";
export type FixLocation = "Software" | "Bios" | "Hardware";
export type BiosPriority = "Critical" | "Recommended" | "Optional";

export const SECTION_LABELS: Record<Section, string> = {
  Controller: "Controller",
  KeyboardMouse: "Keyboard & Mouse",
  Network: "Network & Ping",
  Gpu: "GPU",
  Cpu: "CPU",
  Memory: "Memory",
  Storage: "Storage",
  System: "System",
  Latency: "Latency & DPC",
  Fortnite: "Fortnite",
};

export const SECTION_ORDER: Section[] = [
  "Controller",
  "KeyboardMouse",
  "Network",
  "Gpu",
  "Cpu",
  "Memory",
  "Storage",
  "System",
  "Latency",
  "Fortnite",
];

// ---------------------------------------------------------------------------
// Hardware
// ---------------------------------------------------------------------------

export interface Cpu {
  name: string;
  vendor: string;
  physical_cores: number;
  logical_threads: number;
  base_mhz: number;
  max_mhz: number;
  generation: number | null;
  hybrid_architecture: boolean;
  l3_cache_kb: number;
  has_3d_vcache: boolean;
}

export interface Gpu {
  name: string;
  vendor: string;
  vram_mb: number;
  driver_version: string;
  is_integrated: boolean;
}

export interface MemoryInfo {
  total_mb: number;
  configured_mhz: number;
  rated_mhz: number;
  module_count: number;
}

export interface StorageDevice {
  model: string;
  media_type: string;
  size_gb: number;
  is_system_drive: boolean;
  hosts_fortnite: boolean;
}

export interface NetworkAdapter {
  name: string;
  description: string;
  is_wifi: boolean;
  is_connected: boolean;
  is_default_route: boolean;
  link_speed_mbps: number;
}

export interface DisplayInfo {
  name: string;
  width: number;
  height: number;
  refresh_hz: number;
  max_refresh_hz: number;
  is_primary: boolean;
}

export interface Controller {
  name: string;
  kind: string;
  connection: string;
}

export interface InputDevice {
  name: string;
  is_wireless: boolean;
}

export interface Peripherals {
  mice: InputDevice[];
  keyboards: InputDevice[];
  controllers: Controller[];
  pointer_precision_enabled: boolean;
  filter_keys_enabled: boolean;
}

export interface OperatingSystem {
  caption: string;
  build: number;
  display_version: string;
  vbs_enabled: boolean;
  hvci_enabled: boolean;
  hags_enabled: boolean;
  is_windows_11: boolean;
}

export interface Motherboard {
  manufacturer: string;
  product: string;
  bios_version: string;
}

export interface FortniteInstall {
  found: boolean;
  install_path: string | null;
}

export interface HardwareProfile {
  cpu: Cpu;
  gpus: Gpu[];
  memory: MemoryInfo;
  storage: StorageDevice[];
  network: NetworkAdapter[];
  displays: DisplayInfo[];
  os: OperatingSystem;
  peripherals: Peripherals;
  motherboard: Motherboard;
  fortnite: FortniteInstall;
  power: { is_laptop: boolean; active_scheme_name: string };
}

export interface Finding {
  severity: Severity;
  title: string;
  detail: string;
  fix_location: FixLocation;
}

// ---------------------------------------------------------------------------
// Catalog and plan
// ---------------------------------------------------------------------------

export interface TweakMeta {
  id: string;
  name: string;
  section: Section;
  summary: string;
  rationale: string;
  risk: Risk;
  impact: Impact;
  evidence: Evidence;
  requires_reboot: boolean;
  tradeoff: string | null;
}

export interface SelectedTweak {
  id: string;
  reason: string;
}

export interface RejectedTweak {
  id: string;
  reason: string;
}

export interface BiosRecommendation {
  setting: string;
  target_value: string;
  reason: string;
  where_to_find: string;
  priority: BiosPriority;
}

export interface GameSetting {
  setting: string;
  value: string;
  reason: string;
}

export interface OptimisationPlan {
  summary: string;
  selected: SelectedTweak[];
  rejected: RejectedTweak[];
  bios: BiosRecommendation[];
  game_settings: GameSetting[];
  hardware_notes: string[];
}

export interface ValidationReport {
  unknown_ids: string[];
  duplicates: string[];
  not_applicable: string[];
}

export interface PlanResult {
  plan: OptimisationPlan;
  validation: ValidationReport;
  offline: boolean;
  /** Why the offline fallback was used. Null when the AI plan succeeded. */
  fallback_reason: string | null;
}

export interface PlannedChange {
  tweak_id: string;
  tweak_name: string;
  description: string;
  would_change: boolean;
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

export interface AppliedChange {
  id: string;
  name: string;
  reason: string;
  evidence: Evidence;
  expected_to_help: boolean;
  tradeoff: string | null;
}

export interface SectionReport {
  section: Section;
  label: string;
  applied: AppliedChange[];
  effective_count: number;
  neutral_count: number;
}

export interface FailureDetail {
  id: string;
  name: string;
  message: string;
}

export interface Report {
  machine: string;
  run_id: string;
  summary: string;
  effective_changes: number;
  neutral_changes: number;
  skipped: number;
  failed: number;
  reboot_required: boolean;
  restore_point_created: boolean;
  sections: SectionReport[];
  findings: Finding[];
  failures: FailureDetail[];
  bios_markdown: string;
  game_settings: GameSetting[];
  hardware_notes: string[];
}

export interface Environment {
  version: string;
  elevated: boolean;
  has_api_key: boolean;
  catalog_size: number;
  pending_rollback: string | null;
}

export interface JournalSummary {
  run_id: string;
  description: string;
  applied: number;
  reverted: boolean;
}

export interface RevertReport {
  run_id: string;
  restored: number;
  failed: string[];
  reboot_recommended: boolean;
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

export const api = {
  environment: () => invoke<Environment>("get_environment"),
  saveApiKey: (key: string) => invoke<void>("save_api_key", { key }),
  clearApiKey: () => invoke<void>("clear_api_key"),
  maskedApiKey: () => invoke<string | null>("get_masked_api_key"),

  scan: () => invoke<HardwareProfile>("run_scan"),
  catalog: () => invoke<TweakMeta[]>("get_catalog"),

  buildPlan: () => invoke<PlanResult>("build_plan"),
  preview: () => invoke<PlannedChange[]>("preview_plan"),
  apply: (tweakIds?: string[]) => invoke<Report>("apply_plan", { tweakIds }),

  runs: () => invoke<JournalSummary[]>("list_runs"),
  rollback: (runId?: string) => invoke<RevertReport>("rollback_run", { runId }),
  exportReport: (report: Report) => invoke<string>("export_report", { report }),
};

/** Tauri surfaces command errors as plain strings. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
