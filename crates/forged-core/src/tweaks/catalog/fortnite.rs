//! Fortnite.
//!
//! **Everything here is operating-system configuration that happens to name the
//! game's executable. Nothing in Forged reads, writes, injects into, hooks, or
//! otherwise touches the Fortnite process.** Easy Anti-Cheat treats process
//! interference as a ban condition, and it is right to.
//!
//! The distinction matters and is worth being precise about: setting an Image
//! File Execution Options priority class tells the *Windows loader* what
//! priority to start a process at. It is the same mechanism Task Manager uses,
//! it happens before the process exists, and it is indistinguishable from a user
//! setting priority by hand. That is categorically different from writing to a
//! running game's memory.

use crate::hardware::HardwareProfile;
use crate::tweaks::model::*;

const IFEO: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";

/// The shipping executable name. Stable across seasons.
const FORTNITE_EXE: &str = "FortniteClient-Win64-Shipping.exe";

fn priority_actions(_profile: &HardwareProfile) -> Vec<Action> {
    let path = format!(r"{IFEO}\{FORTNITE_EXE}\PerfOptions");
    vec![
        // CpuPriorityClass 3 = High. Not Realtime (6): Realtime outranks kernel
        // input and audio threads and reliably makes the machine feel worse.
        hklm_dword(&path, "CpuPriorityClass", 3),
        hklm_dword(&path, "IoPriority", 3),
    ]
}

fn gpu_preference_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(exe) = profile.fortnite.executable_path.as_ref() else {
        return Vec::new();
    };
    // GpuPreference=2 requests the high-performance adapter.
    vec![Action::SetRegistry {
        hive: Hive::CurrentUser,
        path: r"Software\Microsoft\DirectX\UserGpuPreferences".into(),
        value: exe.clone(),
        data: RegData::Sz("GpuPreference=2;".into()),
    }]
}

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "fortnite.process_priority",
        name: "Launch Fortnite at high priority",
        section: Section::Fortnite,
        summary: "Tells the Windows loader to start the game process at High priority class.",
        rationale: "The game's threads win scheduler contention against background work without \
                    having to wait for a time slice. High rather than Realtime is deliberate: \
                    Realtime outranks kernel input and audio processing, which makes the machine \
                    stutter and the mouse feel worse, not better.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: priority_actions,
    },
    Tweak {
        id: "fortnite.gpu_preference",
        name: "Pin Fortnite to the high-performance GPU",
        section: Section::Fortnite,
        summary: "Registers a per-application graphics preference for the game executable.",
        rationale: "On a system with integrated graphics alongside a discrete card, Windows \
                    occasionally hands the game the integrated adapter after a driver update. \
                    Pinning the preference makes that impossible.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| p.fortnite.executable_path.is_some() && p.gpus.len() > 1,
        build: gpu_preference_actions,
    },
    Tweak {
        id: "fortnite.disable_fullscreen_optimisations",
        name: "Set the game's fullscreen compatibility flags",
        section: Section::Fortnite,
        summary: "Writes an AppCompat layer entry opting the executable out of the fullscreen \
                  optimisation shim.",
        rationale: "Applies the fullscreen behaviour choice to Fortnite specifically rather than \
                    system-wide, so the rest of the machine is unaffected. Only applied when the \
                    scan found the executable, since the flag is keyed on its full path.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: false,
        tradeoff: Some(
            "If you enable NVIDIA Reflex in the game's settings, revert this — Reflex \
                        performs better on the fullscreen-optimisation path.",
        ),
        applies_to: |p| p.fortnite.executable_path.is_some(),
        build: |p| {
            let Some(exe) = p.fortnite.executable_path.as_ref() else {
                return Vec::new();
            };
            vec![Action::SetRegistry {
                hive: Hive::CurrentUser,
                path: r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers".into(),
                value: exe.clone(),
                data: RegData::Sz("~ DISABLEDXMAXIMIZEDWINDOWEDMODE".into()),
            }]
        },
    },
    Tweak {
        id: "fortnite.epic_launcher_startup",
        name: "Stop the Epic launcher starting with Windows",
        section: Section::Fortnite,
        summary: "Removes the Epic Games Launcher from the startup run key.",
        rationale: "The launcher sits resident checking for updates and running its store \
                    front-end. You need it to start the game, not to start with the machine.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("Cloud saves sync when you next open the launcher rather than at boot."),
        applies_to: always,
        build: |_| {
            vec![
                Action::DeleteRegistryValue {
                    hive: Hive::CurrentUser,
                    path: r"Software\Microsoft\Windows\CurrentVersion\Run".into(),
                    value: "EpicGamesLauncher".into(),
                },
                Action::DeleteRegistryValue {
                    hive: Hive::LocalMachine,
                    path: r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run".into(),
                    value: "EpicGamesLauncher".into(),
                },
            ]
        },
    },
    Tweak {
        id: "fortnite.shader_cache_exclusion",
        name: "Protect the shader cache from cleanup",
        section: Section::Fortnite,
        summary: "Excludes the DirectX and vendor shader cache directories from automatic disk \
                  cleanup.",
        rationale: "Fortnite compiles a large shader set that is invalidated by every driver \
                    update and every season patch. When a cleanup tool deletes the cache, the \
                    game recompiles shaders during your next match — which is exactly the \
                    hitching people blame on their hardware.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            let path = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches\DirectX Shader Cache";
            vec![hklm_dword(path, "Autorun", 0)]
        },
    },
    Tweak {
        id: "fortnite.easyanticheat_service",
        name: "Keep the anti-cheat service healthy",
        section: Section::Fortnite,
        summary: "Ensures the Easy Anti-Cheat service is set to manual start rather than disabled.",
        rationale:
            "Debloat scripts frequently disable EasyAntiCheat because they do not recognise \
                    the service name. With it disabled, Fortnite will not launch at all. This \
                    entry exists to repair that damage — and it is the clearest illustration of \
                    Forged's position on anti-cheat: the correct thing to do is leave it \
                    completely alone and working.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                service("EasyAntiCheat", ServiceStart::Manual),
                service("BEService", ServiceStart::Manual),
            ]
        },
    },
    Tweak {
        id: "fortnite.disable_game_bar_for_game",
        name: "Disable the Game Bar overlay for Fortnite",
        section: Section::Fortnite,
        summary: "Marks the executable as not a game for Game Bar purposes.",
        rationale: "Stops the overlay attaching to the render loop. The overlay is a legitimate \
                    frame-time cost even when it is not visible, because it still hooks present.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: Some(
            "Windows-native clip capture and the FPS counter stop working for \
                        Fortnite.",
        ),
        applies_to: always,
        build: |_| {
            vec![hkcu_dword(
                r"System\GameConfigStore\Children",
                "GameDVR_Enabled",
                0,
            )]
        },
    },
];
