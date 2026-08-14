//! The tweak type system.
//!
//! A `Tweak` is a *declaration*, never an executor. It describes what should be
//! true of the machine and how to get there; the engine in `super::engine` is
//! the only thing that touches the OS, and it captures prior state for every
//! action before performing it. That split is what makes the whole catalog
//! reversible by construction rather than by discipline.

use crate::hardware::HardwareProfile;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// UI grouping. Mirrors the sidebar in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Section {
    /// Gamepad latency, polling, wireless power management.
    Controller,
    /// Mouse acceleration, polling, keyboard repeat, accessibility interference.
    KeyboardMouse,
    /// Ping, jitter, packet scheduling, NIC offloads.
    Network,
    Gpu,
    Cpu,
    Memory,
    Storage,
    /// Background services, telemetry, shell overhead.
    System,
    /// DPC latency, interrupt handling, timer behaviour.
    Latency,
    /// Fortnite process and config specific.
    Fortnite,
}

impl Section {
    pub fn label(&self) -> &'static str {
        match self {
            Section::Controller => "Controller",
            Section::KeyboardMouse => "Keyboard & Mouse",
            Section::Network => "Network & Ping",
            Section::Gpu => "GPU",
            Section::Cpu => "CPU",
            Section::Memory => "Memory",
            Section::Storage => "Storage",
            Section::System => "System",
            Section::Latency => "Latency & DPC",
            Section::Fortnite => "Fortnite",
        }
    }

    pub fn all() -> &'static [Section] {
        &[
            Section::Controller,
            Section::KeyboardMouse,
            Section::Network,
            Section::Gpu,
            Section::Cpu,
            Section::Memory,
            Section::Storage,
            Section::System,
            Section::Latency,
            Section::Fortnite,
        ]
    }
}

/// What happens if this tweak is wrong for the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Risk {
    /// Cosmetic or trivially reversible; worst case is a preference change.
    Low,
    /// Changes system behaviour outside games, or reduces a convenience feature.
    Medium,
    /// Reduces security posture, disables a safety net, or can require Safe Mode
    /// to undo if the machine reacts badly. Always disclosed in the UI.
    High,
}

/// Expected magnitude on a Fortnite-only machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Impact {
    /// Single-digit-percent or better FPS, or >2 ms of latency.
    Major,
    /// Measurable but small; matters most in aggregate.
    Moderate,
    /// Below the noise floor individually. Included because the stack of them
    /// adds up and because their cost is zero.
    Minor,
}

/// How well-supported the claim behind this tweak actually is.
///
/// This field exists because most "gaming optimisation" lists are 60% folklore.
/// Forged ships the folklore entries too — with this flag set — so the report
/// can be honest about which changes are doing the work. Nothing is presented as
/// a performance win unless it is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Evidence {
    /// Reproducible in third-party or first-party benchmarks.
    Measured,
    /// Follows directly from documented Windows behaviour, even if the exact
    /// magnitude varies by machine.
    Documented,
    /// Frees resources whose contention is machine-specific. Helps on loaded
    /// systems, does nothing on clean ones.
    SituationalGain,
    /// Widely recommended online, no reproducible benefit. Applied only because
    /// it is harmless and users expect to see it; reported as neutral.
    NoMeasuredBenefit,
}

impl Evidence {
    /// Whether this entry should be counted in the report's headline claims.
    pub fn counts_toward_gains(&self) -> bool {
        !matches!(self, Evidence::NoMeasuredBenefit)
    }
}

// ---------------------------------------------------------------------------
// Registry primitives
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
    Users,
    ClassesRoot,
}

impl Hive {
    pub fn short_name(&self) -> &'static str {
        match self {
            Hive::LocalMachine => "HKLM",
            Hive::CurrentUser => "HKCU",
            Hive::Users => "HKU",
            Hive::ClassesRoot => "HKCR",
        }
    }
}

/// A registry value payload. Mirrors the REG_* types Forged actually needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum RegData {
    Dword(u32),
    Qword(u64),
    Sz(String),
    ExpandSz(String),
    MultiSz(Vec<String>),
    Binary(Vec<u8>),
}

impl RegData {
    pub fn describe(&self) -> String {
        match self {
            RegData::Dword(v) => format!("DWORD 0x{v:08X} ({v})"),
            RegData::Qword(v) => format!("QWORD {v}"),
            RegData::Sz(s) => format!("\"{s}\""),
            RegData::ExpandSz(s) => format!("\"{s}\" (expandable)"),
            RegData::MultiSz(v) => format!("[{}]", v.join(", ")),
            RegData::Binary(b) => format!("{} bytes", b.len()),
        }
    }
}

/// Windows service start type, matching the SERVICE_*_START constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceStart {
    Boot = 0,
    System = 1,
    Automatic = 2,
    Manual = 3,
    Disabled = 4,
}

impl ServiceStart {
    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            0 => ServiceStart::Boot,
            1 => ServiceStart::System,
            2 => ServiceStart::Automatic,
            3 => ServiceStart::Manual,
            4 => ServiceStart::Disabled,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// A single reversible operation against the OS.
///
/// Registry and service actions carry no revert payload because the engine
/// reads the prior state immediately before writing. External commands cannot
/// be introspected that way, so they must declare their own inverse — and the
/// engine refuses to run a command action that does not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    SetRegistry {
        hive: Hive,
        path: String,
        value: String,
        data: RegData,
    },
    /// Used where the *absence* of a value is the desired state.
    DeleteRegistryValue {
        hive: Hive,
        path: String,
        value: String,
    },
    SetServiceStart {
        service: String,
        start: ServiceStart,
    },
    /// Shell out to powercfg / netsh / bcdedit / PowerShell.
    RunCommand {
        program: String,
        args: Vec<String>,
        /// The inverse invocation. Mandatory — see type docs.
        revert: RevertCommand,
        /// Non-zero exit codes to treat as success (e.g. "already set").
        tolerate_exit_codes: Vec<i32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl RevertCommand {
    pub fn new(program: impl Into<String>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Action {
    /// Human-readable one-liner for the report and the pre-apply confirmation.
    pub fn describe(&self) -> String {
        match self {
            Action::SetRegistry {
                hive,
                path,
                value,
                data,
            } => format!(
                "set {}\\{}\\{} = {}",
                hive.short_name(),
                path,
                value,
                data.describe()
            ),
            Action::DeleteRegistryValue { hive, path, value } => {
                format!("delete {}\\{}\\{}", hive.short_name(), path, value)
            }
            Action::SetServiceStart { service, start } => {
                format!("service {service} start type -> {start:?}")
            }
            Action::RunCommand { program, args, .. } => {
                format!("{} {}", program, args.join(" "))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The tweak itself
// ---------------------------------------------------------------------------

/// Builds the concrete actions for a machine. Taking the profile lets an entry
/// target a specific NIC GUID or GPU class key rather than hard-coding paths.
pub type ActionBuilder = fn(&HardwareProfile) -> Vec<Action>;

/// Decides whether this entry is relevant to the machine at all.
pub type Applicability = fn(&HardwareProfile) -> bool;

pub struct Tweak {
    /// Stable identifier. This is the *only* thing the AI planner is allowed to
    /// return, and it is validated against the catalog before execution.
    pub id: &'static str,
    pub name: &'static str,
    pub section: Section,
    /// What it changes, in plain language.
    pub summary: &'static str,
    /// Why it helps Fortnite specifically. Shown in the report.
    pub rationale: &'static str,
    pub risk: Risk,
    pub impact: Impact,
    pub evidence: Evidence,
    pub requires_reboot: bool,
    /// Disclosed prominently when `risk` is High.
    pub tradeoff: Option<&'static str>,
    pub applies_to: Applicability,
    pub build: ActionBuilder,
}

impl Tweak {
    pub fn is_relevant(&self, profile: &HardwareProfile) -> bool {
        (self.applies_to)(profile)
    }

    pub fn actions_for(&self, profile: &HardwareProfile) -> Vec<Action> {
        (self.build)(profile)
    }

    /// Serialisable view handed to the AI planner and the UI. Deliberately omits
    /// the concrete actions: the planner selects by ID and reasoning, and must
    /// never be in a position to think it can author registry paths.
    pub fn metadata(&self) -> TweakMeta {
        TweakMeta {
            id: self.id.to_string(),
            name: self.name.to_string(),
            section: self.section,
            summary: self.summary.to_string(),
            rationale: self.rationale.to_string(),
            risk: self.risk,
            impact: self.impact,
            evidence: self.evidence,
            requires_reboot: self.requires_reboot,
            tradeoff: self.tradeoff.map(|s| s.to_string()),
        }
    }
}

impl std::fmt::Debug for Tweak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tweak")
            .field("id", &self.id)
            .field("section", &self.section)
            .field("risk", &self.risk)
            .field("impact", &self.impact)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TweakMeta {
    pub id: String,
    pub name: String,
    pub section: Section,
    pub summary: String,
    pub rationale: String,
    pub risk: Risk,
    pub impact: Impact,
    pub evidence: Evidence,
    pub requires_reboot: bool,
    pub tradeoff: Option<String>,
}

// ---------------------------------------------------------------------------
// Small builder helpers, to keep the catalog readable
// ---------------------------------------------------------------------------

/// `HKLM` DWORD set — by far the most common action shape.
pub fn hklm_dword(path: &str, value: &str, data: u32) -> Action {
    Action::SetRegistry {
        hive: Hive::LocalMachine,
        path: path.to_string(),
        value: value.to_string(),
        data: RegData::Dword(data),
    }
}

/// `HKCU` DWORD set.
pub fn hkcu_dword(path: &str, value: &str, data: u32) -> Action {
    Action::SetRegistry {
        hive: Hive::CurrentUser,
        path: path.to_string(),
        value: value.to_string(),
        data: RegData::Dword(data),
    }
}

/// `HKCU` string set — used for the mouse and keyboard control-panel values,
/// which Windows stores as REG_SZ even though they are numeric.
pub fn hkcu_sz(path: &str, value: &str, data: &str) -> Action {
    Action::SetRegistry {
        hive: Hive::CurrentUser,
        path: path.to_string(),
        value: value.to_string(),
        data: RegData::Sz(data.to_string()),
    }
}

pub fn hklm_sz(path: &str, value: &str, data: &str) -> Action {
    Action::SetRegistry {
        hive: Hive::LocalMachine,
        path: path.to_string(),
        value: value.to_string(),
        data: RegData::Sz(data.to_string()),
    }
}

pub fn service(name: &str, start: ServiceStart) -> Action {
    Action::SetServiceStart {
        service: name.to_string(),
        start,
    }
}

/// A command with its mandatory inverse.
pub fn command(program: &str, args: &[&str], revert_args: &[&str]) -> Action {
    Action::RunCommand {
        program: program.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        revert: RevertCommand::new(program, revert_args),
        tolerate_exit_codes: Vec::new(),
    }
}

/// Applicability predicate meaning "always relevant".
pub fn always(_: &HardwareProfile) -> bool {
    true
}
