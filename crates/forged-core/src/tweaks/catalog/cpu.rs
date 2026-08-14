//! CPU and power.
//!
//! The two entries that matter most here are the power plan and
//! `Win32PrioritySeparation`. Most of the rest is small. One entry — disabling
//! VBS — is the single largest software FPS gain available on a stock Windows 11
//! install, and is also the one genuine security tradeoff in the catalog; it is
//! marked High risk and states plainly what it costs.

use crate::tweaks::model::*;

/// The processor power settings subgroup GUID.
const SUB_PROCESSOR: &str = "54533251-82be-4824-96c1-47b60b740d00";

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "cpu.ultimate_performance_plan",
        name: "Activate the Ultimate Performance power plan",
        section: Section::Cpu,
        summary: "Creates and activates Windows' hidden Ultimate Performance scheme.",
        rationale: "Balanced ramps clocks up in response to load, so every load spike begins at a \
                    low clock. Ultimate Performance removes the ramp entirely. This is the single \
                    highest-value power change on the machine and everything else in this section \
                    builds on it.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: Some(
            "Substantially higher idle power draw. On a desktop that exists to play \
                        one game this is the correct trade; on a laptop on battery it is not.",
        ),
        applies_to: |p| !p.power.is_laptop,
        build: |_| {
            vec![
                Action::RunCommand {
                    program: "powercfg.exe".into(),
                    args: vec![
                        "-duplicatescheme".into(),
                        "e9a42b02-d5df-448d-aa00-03f14749eb61".into(),
                    ],
                    revert: RevertCommand::new("powercfg.exe", &["/setactive", "SCHEME_BALANCED"]),
                    // Non-zero when the scheme already exists, which is fine.
                    tolerate_exit_codes: vec![1, 2],
                },
                Action::RunCommand {
                    program: "powercfg.exe".into(),
                    args: vec![
                        "/setactive".into(),
                        "e9a42b02-d5df-448d-aa00-03f14749eb61".into(),
                    ],
                    revert: RevertCommand::new("powercfg.exe", &["/setactive", "SCHEME_BALANCED"]),
                    tolerate_exit_codes: vec![],
                },
            ]
        },
    },
    Tweak {
        id: "cpu.disable_core_parking",
        name: "Disable CPU core parking",
        section: Section::Cpu,
        summary: "Forces all cores to stay online rather than parking idle ones.",
        rationale: "A parked core takes time to come back. When Fortnite's worker threads spin up \
                    for a build fight, unparking latency shows up as a frame-time spike at exactly \
                    the wrong moment.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: Some("Higher idle power and temperatures."),
        applies_to: |p| !p.power.is_laptop,
        build: |_| {
            vec![
                command(
                    "powercfg.exe",
                    &[
                        "/setacvalueindex",
                        "SCHEME_CURRENT",
                        SUB_PROCESSOR,
                        "0cc5b647-c1df-4637-891a-dec35c318583",
                        "100",
                    ],
                    &[
                        "/setacvalueindex",
                        "SCHEME_CURRENT",
                        SUB_PROCESSOR,
                        "0cc5b647-c1df-4637-891a-dec35c318583",
                        "0",
                    ],
                ),
                command(
                    "powercfg.exe",
                    &["/setactive", "SCHEME_CURRENT"],
                    &["/setactive", "SCHEME_CURRENT"],
                ),
            ]
        },
    },
    Tweak {
        id: "cpu.minimum_processor_state",
        name: "Hold the minimum processor state at 100%",
        section: Section::Cpu,
        summary: "Sets the minimum processor performance state to 100% on AC power.",
        rationale: "Removes downclocking entirely, so the CPU is already at full speed when a \
                    frame needs it rather than ramping into it.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: Some(
            "The CPU runs hot and loud at idle. Expect the desktop to sit 15-25 °C \
                        warmer than stock.",
        ),
        applies_to: |p| !p.power.is_laptop,
        build: |_| {
            vec![command(
                "powercfg.exe",
                &[
                    "/setacvalueindex",
                    "SCHEME_CURRENT",
                    SUB_PROCESSOR,
                    "893dee8e-2bef-41e0-89c6-b55d0929964c",
                    "100",
                ],
                &[
                    "/setacvalueindex",
                    "SCHEME_CURRENT",
                    SUB_PROCESSOR,
                    "893dee8e-2bef-41e0-89c6-b55d0929964c",
                    "5",
                ],
            )]
        },
    },
    Tweak {
        id: "cpu.priority_separation",
        name: "Tune the foreground scheduling quantum",
        section: Section::Cpu,
        summary: "Sets Win32PrioritySeparation to short, variable quanta with a 3:1 foreground \
                  boost.",
        rationale: "Controls how much CPU time the foreground window gets relative to background \
                    processes. The value 0x26 gives short time slices — so a thread that blocks \
                    yields quickly — with a strong foreground bias. This is the standard \
                    configuration for latency-sensitive foreground work.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\PriorityControl",
                "Win32PrioritySeparation",
                0x26,
            )]
        },
    },
    // Removed: cpu.disable_vbs. It was the single largest software FPS gain, but
    // it turns off Memory Integrity — a real reduction in the machine's security.
    // A safe-by-default tool should not silently weaken kernel protection, so it
    // is gone rather than merely gated. The BIOS sheet still mentions VBS for
    // anyone who wants to make that call themselves in firmware.
    Tweak {
        id: "cpu.disable_power_throttling",
        name: "Disable per-process power throttling",
        section: Section::Cpu,
        summary: "Turns off the EcoQoS power throttling framework.",
        rationale: "Windows throttles processes it judges to be background work. It occasionally \
                    misclassifies a game's worker threads — particularly on hybrid CPUs where it \
                    also decides which threads land on efficiency cores.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\Power\PowerThrottling",
                "PowerThrottlingOff",
                1,
            )]
        },
    },
    Tweak {
        id: "cpu.mmcss_games_profile",
        name: "Raise the multimedia scheduler's game profile",
        section: Section::Cpu,
        summary: "Increases the GPU and CPU priority the multimedia class scheduler grants to \
                  games.",
        rationale:
            "MMCSS grants scheduling guarantees to registered multimedia tasks. Raising the \
                    Games profile means the game's render thread wins contention against \
                    background work.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            let path = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games";
            vec![
                hklm_dword(path, "GPU Priority", 8),
                hklm_dword(path, "Priority", 6),
                hklm_sz(path, "Scheduling Category", "High"),
                hklm_sz(path, "SFIO Priority", "High"),
                hklm_dword(path, "Clock Rate", 10000),
            ]
        },
    },
    Tweak {
        id: "cpu.remove_platform_clock_override",
        name: "Remove forced HPET platform clock",
        section: Section::Cpu,
        summary: "Deletes the useplatformclock boot flag if some earlier tool set it.",
        rationale: "Forcing the High Precision Event Timer as the system clock source was useful \
                    a decade ago and is actively harmful now — it is markedly slower to read than \
                    the invariant TSC modern CPUs provide. Many tweaking guides still recommend \
                    enabling it. This entry removes it so Windows picks the best source itself.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![Action::RunCommand {
                program: "bcdedit.exe".into(),
                args: vec!["/deletevalue".into(), "useplatformclock".into()],
                revert: RevertCommand::new("bcdedit.exe", &["/deletevalue", "useplatformclock"]),
                // Non-zero when the value was already absent, which is the goal.
                tolerate_exit_codes: vec![1, -1],
            }]
        },
    },
    Tweak {
        id: "cpu.disable_dynamic_tick",
        name: "Disable the dynamic timer tick",
        section: Section::Cpu,
        summary: "Stops the kernel from suppressing timer interrupts during idle periods.",
        rationale:
            "Dynamic tick saves power by skipping timer interrupts when nothing needs them. \
                    Coming out of a tickless period adds a small, variable delay. The effect is \
                    real but small, and varies enough between machines that it is worth measuring \
                    rather than assuming.",
        risk: Risk::Medium,
        impact: Impact::Minor,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: Some(
            "Noticeably higher idle power draw. Revert this one first if the machine \
                        runs hotter than you like.",
        ),
        applies_to: |p| !p.power.is_laptop,
        build: |_| {
            vec![Action::RunCommand {
                program: "bcdedit.exe".into(),
                args: vec!["/set".into(), "disabledynamictick".into(), "yes".into()],
                revert: RevertCommand::new("bcdedit.exe", &["/set", "disabledynamictick", "no"]),
                tolerate_exit_codes: vec![],
            }]
        },
    },
    Tweak {
        id: "cpu.hybrid_scheduler_hint",
        name: "Keep the hybrid core scheduler enabled",
        section: Section::Cpu,
        summary: "Ensures Intel Thread Director scheduling stays on for P-core/E-core CPUs.",
        rationale:
            "Guides written for older CPUs recommend disabling the heterogeneous scheduling \
                    policy. On a 12th-gen or newer Intel chip that is actively harmful: without \
                    Thread Director, Windows parks the game's render thread on an efficiency core \
                    and costs you 20% or more. This entry protects against that advice.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| p.cpu.is_intel_hybrid(),
        build: |_| {
            vec![
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Control\Session Manager\Kernel",
                    "ThreadDirectorEnable",
                    1,
                ),
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Control\Power",
                    "HeteroSystemPolicy",
                    0,
                ),
            ]
        },
    },
    // Removed: cpu.disable_idle_states. Pinning the CPU out of every C-state
    // makes it run at near-full power permanently, and on a cooler that cannot
    // absorb that it thermally throttles under load — losing more than it gains,
    // exactly when a match is loading. This is the entry most likely to make a
    // machine feel worse rather than better, so a safe-by-default tool omits it.
    Tweak {
        id: "cpu.disable_hibernation",
        name: "Disable hibernation and fast startup",
        section: Section::Cpu,
        summary: "Turns off hibernation, which also disables Fast Startup, and reclaims the \
                  hiberfil.sys file.",
        rationale: "Fast Startup does not do a full shutdown — it hibernates the kernel session. \
                    Driver and registry changes therefore do not fully apply on the next boot, \
                    which is why 'I rebooted and the tweak didn't take' happens. Disabling it \
                    makes shutdown mean shutdown, and frees several gigabytes.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("Cold boot takes a few seconds longer."),
        applies_to: |p| !p.power.is_laptop,
        build: |_| {
            vec![command(
                "powercfg.exe",
                &["/hibernate", "off"],
                &["/hibernate", "on"],
            )]
        },
    },
    Tweak {
        id: "cpu.distribute_timer_interrupts",
        name: "Distribute timer interrupts across cores",
        section: Section::Cpu,
        summary: "Stops all periodic timer interrupts landing on core 0.",
        rationale: "By default core 0 services the system timer alongside whatever the scheduler \
                    puts there. Spreading the load removes a contention point that shows up as \
                    DPC latency.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| p.cpu.logical_threads >= 8,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\Session Manager\Kernel",
                "DistributeTimers",
                1,
            )]
        },
    },
    Tweak {
        id: "cpu.disable_spectre_mitigations",
        name: "Speculative execution mitigations",
        section: Section::Cpu,
        summary: "Leaves Spectre and Meltdown mitigations enabled.",
        rationale: "Disabling CPU vulnerability mitigations is a popular tweak that does return a \
                    few percent on older silicon. Forged does not do it and does not offer it: on \
                    any CPU from the last five years the hardware fixes mean the software \
                    mitigation costs almost nothing, so the trade is a real vulnerability for \
                    approximately zero frames. Listed so you know it was considered and rejected.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::NoMeasuredBenefit,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| Vec::new(),
    },
];
