//! GPU.
//!
//! Hardware Accelerated GPU Scheduling is handled as two mutually exclusive
//! entries rather than one, because the correct answer genuinely differs by
//! vendor and generation: NVIDIA Reflex depends on HAGS to deliver its full
//! latency reduction, while on older and AMD hardware HAGS is at best neutral
//! and at worst a small regression. The applicability predicates make exactly
//! one of the pair relevant to any given machine.

use crate::hardware::{GpuVendor, HardwareProfile};
use crate::tweaks::model::*;

/// The display class key node for the primary GPU.
fn gpu_class_path(profile: &HardwareProfile) -> Option<String> {
    let gpu = profile.primary_gpu()?;
    let index = gpu.class_key_index.as_ref()?;
    Some(format!(
        r"SYSTEM\CurrentControlSet\Control\Class\{{4d36e968-e325-11ce-bfc1-08002be10318}}\{index}"
    ))
}

fn is_nvidia(profile: &HardwareProfile) -> bool {
    profile
        .primary_gpu()
        .is_some_and(|g| g.vendor == GpuVendor::Nvidia)
}

fn is_amd(profile: &HardwareProfile) -> bool {
    profile
        .primary_gpu()
        .is_some_and(|g| g.vendor == GpuVendor::Amd)
}

/// HAGS is worth having on NVIDIA RTX hardware, where Reflex leans on it.
fn hags_is_beneficial(profile: &HardwareProfile) -> bool {
    profile.primary_gpu().is_some_and(|g| {
        g.vendor == GpuVendor::Nvidia
            && (g.name.contains("RTX") || g.name.contains("40") || g.name.contains("50"))
    })
}

fn nvidia_power_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(path) = gpu_class_path(profile) else {
        return Vec::new();
    };
    vec![
        // PerfLevelSrc 0x2222: ignore the driver's own AC/DC heuristics.
        hklm_dword(&path, "PerfLevelSrc", 0x2222),
        hklm_dword(&path, "PowerMizerEnable", 1),
        hklm_dword(&path, "PowerMizerLevel", 1),
        hklm_dword(&path, "PowerMizerLevelAC", 1),
    ]
}

fn amd_ulps_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(path) = gpu_class_path(profile) else {
        return Vec::new();
    };
    vec![
        hklm_dword(&path, "EnableUlps", 0),
        hklm_dword(&path, "EnableUlps_NA", 0),
        hklm_dword(&path, "PP_SclkDeepSleepDisable", 1),
    ]
}

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "gpu.hags_enable",
        name: "Enable Hardware Accelerated GPU Scheduling",
        section: Section::Gpu,
        summary: "Turns HAGS on, letting the GPU manage its own command queue instead of the \
                  CPU driver thread.",
        rationale: "On RTX hardware, NVIDIA Reflex depends on HAGS to reach its full latency \
                    reduction — with HAGS off, Reflex leaves several milliseconds on the table. \
                    Fortnite supports Reflex, so this is worth having on this GPU.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: hags_is_beneficial,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
                "HwSchMode",
                2,
            )]
        },
    },
    Tweak {
        id: "gpu.hags_disable",
        name: "Disable Hardware Accelerated GPU Scheduling",
        section: Section::Gpu,
        summary: "Turns HAGS off and returns command scheduling to the CPU-side driver.",
        rationale: "Outside NVIDIA's Reflex path, HAGS is neutral at best and a measurable \
                    regression on some older and AMD configurations, where it interacts poorly \
                    with the driver's own frame pacing. This GPU is in that group.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| !p.gpus.is_empty() && !hags_is_beneficial(p),
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
                "HwSchMode",
                1,
            )]
        },
    },
    Tweak {
        id: "gpu.disable_game_dvr",
        name: "Disable background game recording",
        section: Section::Gpu,
        summary: "Switches off Game DVR, background capture and the Game Bar capture hooks.",
        rationale: "Game DVR keeps a rolling buffer of the last 30 seconds of gameplay encoded in \
                    the background, whether or not you ever save a clip. It costs frames \
                    continuously for a feature most players never use.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: Some(
            "The Windows-native clip capture shortcut stops working. Use the GPU \
                        vendor's own recorder if you want clips.",
        ),
        applies_to: always,
        build: |_| {
            vec![
                hkcu_dword(r"System\GameConfigStore", "GameDVR_Enabled", 0),
                hkcu_dword(r"System\GameConfigStore", "GameDVR_FSEBehaviorMode", 2),
                hkcu_dword(
                    r"System\GameConfigStore",
                    "GameDVR_HonorUserFSEBehaviorMode",
                    1,
                ),
                hkcu_dword(
                    r"System\GameConfigStore",
                    "GameDVR_DXGIHonorFSEWindowsCompatible",
                    1,
                ),
                hklm_dword(
                    r"SOFTWARE\Policies\Microsoft\Windows\GameDVR",
                    "AllowGameDVR",
                    0,
                ),
                hkcu_dword(
                    r"Software\Microsoft\Windows\CurrentVersion\GameDVR",
                    "AppCaptureEnabled",
                    0,
                ),
            ]
        },
    },
    Tweak {
        id: "gpu.disable_mpo",
        name: "Disable Multiplane Overlay",
        section: Section::Gpu,
        summary: "Sets the DWM overlay test mode that disables MPO.",
        rationale: "MPO is the documented cause of a family of flickering, black-screen and \
                    stutter bugs that both NVIDIA and Microsoft have publicly acknowledged and \
                    issued guidance to disable it for. If you have unexplained flicker when \
                    alt-tabbing, this is usually it.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: Some("Very slightly higher desktop compositor power draw when idle."),
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SOFTWARE\Microsoft\Windows\Dwm",
                "OverlayTestMode",
                5,
            )]
        },
    },
    Tweak {
        id: "gpu.nvidia_power_management",
        name: "Force maximum GPU clocks (NVIDIA)",
        section: Section::Gpu,
        summary: "Sets PowerMizer to prefer maximum performance rather than adaptive clocks.",
        rationale: "Adaptive clocking drops the core clock when it thinks the GPU is idle. In a \
                    game with variable load — Fortnite's open fields versus a build fight — that \
                    produces a clock ramp on every load spike, felt as a stutter at the exact \
                    moment things get busy.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: Some(
            "Higher idle power draw and fan noise on the desktop, since the GPU no \
                        longer downclocks aggressively.",
        ),
        applies_to: |p| is_nvidia(p) && gpu_class_path(p).is_some(),
        build: nvidia_power_actions,
    },
    Tweak {
        id: "gpu.amd_disable_ulps",
        name: "Disable Ultra Low Power State (AMD)",
        section: Section::Gpu,
        summary: "Turns off ULPS and deep sleep on the AMD GPU.",
        rationale: "ULPS drops the card into a very low power state that takes noticeable time to \
                    exit. The AMD equivalent of the NVIDIA clock-ramp stutter above.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: Some("Higher idle power draw and fan noise on the desktop."),
        applies_to: |p| is_amd(p) && gpu_class_path(p).is_some(),
        build: amd_ulps_actions,
    },
    Tweak {
        id: "gpu.tdr_delay",
        name: "Raise the driver timeout threshold",
        section: Section::Gpu,
        summary: "Increases TdrDelay from 2 to 10 seconds.",
        rationale: "Not a performance change. Windows resets the graphics driver if it does not \
                    respond within the timeout, which on a heavily loaded GPU during shader \
                    compilation can trigger a spurious 'display driver stopped responding' crash. \
                    A longer threshold avoids losing a match to a false positive.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some("A genuinely hung GPU takes longer to recover."),
        applies_to: always,
        build: |_| {
            let path = r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers";
            vec![
                hklm_dword(path, "TdrDelay", 10),
                hklm_dword(path, "TdrDdiDelay", 10),
            ]
        },
    },
    Tweak {
        id: "gpu.shader_cache_size",
        name: "Enlarge the DirectX shader cache",
        section: Section::Gpu,
        summary: "Raises the shader disk cache limit so compiled shaders are not evicted.",
        rationale: "Fortnite compiles a large shader set, and every season's update invalidates \
                    part of it. When the cache is too small, shaders are recompiled mid-match — \
                    which is the traversal stutter people blame on their GPU.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some("Uses a few gigabytes more disk space."),
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
                "ShaderCacheSizeMB",
                10240,
            )]
        },
    },
    Tweak {
        id: "gpu.disable_preemption",
        name: "Reduce GPU preemption granularity (NVIDIA)",
        section: Section::Gpu,
        summary: "Disables compute and graphics preemption on the NVIDIA driver.",
        rationale: "Preemption lets the GPU interrupt a running draw call to service another \
                    context. It improves desktop responsiveness under mixed load and costs a \
                    small amount of throughput in a single fullscreen application.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| is_nvidia(p) && gpu_class_path(p).is_some(),
        build: |p| {
            let Some(path) = gpu_class_path(p) else {
                return Vec::new();
            };
            vec![
                hklm_dword(&path, "DisablePreemption", 1),
                hklm_dword(&path, "DisableCudaContextPreemption", 1),
            ]
        },
    },
    Tweak {
        id: "gpu.prefer_high_performance_adapter",
        name: "Force the discrete GPU as default",
        section: Section::Gpu,
        summary: "Sets the system-wide graphics preference to high performance.",
        rationale: "On a machine with both integrated and discrete graphics, Windows sometimes \
                    hands a game the integrated adapter — which on Fortnite is the difference \
                    between 240 FPS and 40. Forcing the preference removes the guesswork.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| p.gpus.len() > 1,
        build: |_| {
            vec![hkcu_dword(
                r"Software\Microsoft\DirectX\UserGpuPreferences",
                "DirectXUserGlobalSettings",
                0,
            )]
        },
    },
    Tweak {
        id: "gpu.disable_fso_hybrid",
        name: "Disable fullscreen optimisations globally",
        section: Section::Gpu,
        summary: "Opts every application out of the borderless-windowed fullscreen shim.",
        rationale: "This one is genuinely contested. Historically FSO added a compositor hop and \
                    disabling it reduced latency. On current Windows 11 with a Reflex-enabled \
                    DX12 title, FSO is the *supported* path and disabling it can cost you Reflex \
                    entirely. Forged applies it only where Reflex is not in play, and says so.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: false,
        tradeoff: Some(
            "If you later enable Reflex in Fortnite, revert this — the two work \
                        against each other.",
        ),
        applies_to: |p| !hags_is_beneficial(p),
        build: |_| {
            vec![
                hkcu_dword(r"System\GameConfigStore", "GameDVR_FSEBehavior", 2),
                hkcu_dword(
                    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers",
                    "DisableFSO",
                    1,
                ),
            ]
        },
    },
    Tweak {
        id: "gpu.disable_hdr_autohdr",
        name: "Disable Auto HDR",
        section: Section::Gpu,
        summary: "Turns off the automatic SDR-to-HDR tone mapping pass.",
        rationale: "Auto HDR runs an extra full-screen pass on every frame. On an SDR monitor it \
                    does nothing visible while still costing the pass.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("If you have an HDR monitor and want Auto HDR, skip this one."),
        applies_to: always,
        // Written as a named value under UserGpuPreferences rather than as the
        // default value of an AutoHDREnable key: a default-value write creates a
        // key that undo cannot remove, which would leave the setting stuck off
        // after a rollback.
        build: |_| {
            vec![
                hkcu_dword(
                    r"Software\Microsoft\DirectX\UserGpuPreferences",
                    "AutoHDREnable",
                    0,
                ),
                hkcu_dword(r"Software\Microsoft\Windows\Dwm", "EnableAutoHDR", 0),
            ]
        },
    },
    Tweak {
        id: "gpu.disable_variable_refresh_stutter_workaround",
        name: "Clear stale display timing overrides",
        section: Section::Gpu,
        summary: "Removes third-party refresh-rate and timing overrides from the graphics driver \
                  key.",
        rationale: "Custom resolution utilities leave timing overrides behind that can silently \
                    cap a 240 Hz panel or introduce frame pacing artefacts long after the tool is \
                    gone.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| gpu_class_path(p).is_some(),
        build: |p| {
            let Some(path) = gpu_class_path(p) else {
                return Vec::new();
            };
            vec![
                Action::DeleteRegistryValue {
                    hive: Hive::LocalMachine,
                    path: path.clone(),
                    value: "DALNonStandardModesBCD1".into(),
                },
                Action::DeleteRegistryValue {
                    hive: Hive::LocalMachine,
                    path,
                    value: "DALR6 DFPMaxRefreshRate".into(),
                },
            ]
        },
    },
];
