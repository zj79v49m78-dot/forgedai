//! Latency & DPC.
//!
//! This section targets the path an interrupt takes from a device to the code
//! that handles it. It is where the difference between "high FPS" and "feels
//! responsive" lives: a machine can render 300 frames a second and still feel
//! sluggish if deferred procedure calls are being serviced late.
//!
//! Message Signaled Interrupts are the centrepiece. Line-based interrupts are
//! shared between devices and must be acknowledged through the interrupt
//! controller; MSI lets a device write its interrupt directly to a CPU, which
//! removes the sharing and the acknowledgement round trip.

use crate::hardware::HardwareProfile;
use crate::tweaks::model::*;

/// Path to a PCI device's MSI configuration node.
fn msi_path(pnp_device_id: &str) -> String {
    format!(
        r"SYSTEM\CurrentControlSet\Enum\{}\Device Parameters\Interrupt Management\MessageSignaledInterruptProperties",
        pnp_device_id
    )
}

/// Path to a PCI device's interrupt priority node.
fn affinity_path(pnp_device_id: &str) -> String {
    format!(
        r"SYSTEM\CurrentControlSet\Enum\{}\Device Parameters\Interrupt Management\Affinity Policy",
        pnp_device_id
    )
}

fn gpu_msi_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(gpu) = profile.primary_gpu() else {
        return Vec::new();
    };
    if gpu.pnp_device_id.is_empty() {
        return Vec::new();
    }
    vec![hklm_dword(&msi_path(&gpu.pnp_device_id), "MSISupported", 1)]
}

/// MSI is keyed on the device instance path, so only adapters whose PnP path the
/// scanner actually resolved are touched. An adapter we cannot address precisely
/// is skipped rather than guessed at.
fn nic_msi_actions(profile: &HardwareProfile) -> Vec<Action> {
    profile
        .network
        .iter()
        .filter(|n| n.is_connected && !n.pnp_device_id.is_empty())
        .map(|n| hklm_dword(&msi_path(&n.pnp_device_id), "MSISupported", 1))
        .collect()
}

fn gpu_interrupt_priority_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(gpu) = profile.primary_gpu() else {
        return Vec::new();
    };
    if gpu.pnp_device_id.is_empty() {
        return Vec::new();
    }
    // DevicePriority 3 = High.
    vec![hklm_dword(
        &affinity_path(&gpu.pnp_device_id),
        "DevicePriority",
        3,
    )]
}

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "latency.gpu_msi_mode",
        name: "Enable Message Signaled Interrupts on the GPU",
        section: Section::Latency,
        summary: "Switches the graphics card from line-based interrupts to MSI.",
        rationale: "Line-based interrupts are shared between devices and require a round trip \
                    through the interrupt controller to work out who raised them. MSI lets the \
                    GPU write its interrupt straight to a CPU core. This is one of the most \
                    reliable DPC latency reductions available and it is why a machine can feel \
                    smoother at the same frame rate.",
        risk: Risk::Medium,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: Some(
            "A small number of older GPUs behave badly with MSI enabled and can \
                        black-screen on boot. If that happens, boot into Safe Mode and use \
                        Forged's rollback, or the restore point taken before this run.",
        ),
        applies_to: |p| p.primary_gpu().is_some_and(|g| !g.pnp_device_id.is_empty()),
        build: gpu_msi_actions,
    },
    Tweak {
        id: "latency.gpu_interrupt_priority",
        name: "Raise GPU interrupt priority",
        section: Section::Latency,
        summary: "Sets the graphics device's interrupt affinity policy to high priority.",
        rationale: "Tells the kernel to service the GPU's interrupts ahead of other devices. \
                    Pairs with MSI: MSI removes the routing overhead, this decides who goes first \
                    when several devices interrupt at once.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: Some(
            "Deprioritises other devices' interrupts, including audio and USB. Revert \
                        if you notice audio crackling.",
        ),
        applies_to: |p| p.primary_gpu().is_some_and(|g| !g.pnp_device_id.is_empty()),
        build: gpu_interrupt_priority_actions,
    },
    Tweak {
        id: "latency.nic_msi_mode",
        name: "Enable Message Signaled Interrupts on the network card",
        section: Section::Latency,
        summary: "Switches the active network adapter to MSI where the device exposes it.",
        rationale: "The same reasoning as the GPU, applied to the device whose interrupt latency \
                    directly becomes your ping jitter.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: Some(
            "As with the GPU, a device that mishandles MSI can fail to initialise. \
                        Rollback restores it.",
        ),
        applies_to: |p| {
            p.network
                .iter()
                .any(|n| n.is_connected && !n.pnp_device_id.is_empty())
        },
        build: nic_msi_actions,
    },
    Tweak {
        id: "latency.global_timer_resolution",
        name: "Grant timer resolution requests globally",
        section: Section::Latency,
        summary: "Sets GlobalTimerResolutionRequests so a resolution request applies system-wide.",
        rationale: "Since Windows 10 2004, a process asking for a high-resolution timer only gets \
                    it for itself, and only while focused. Games rely on high-resolution timers \
                    for frame pacing; this restores the older global behaviour so the improvement \
                    is not lost the moment something else takes focus.",
        risk: Risk::Medium,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: Some("A system-wide 0.5 ms timer increases idle power draw."),
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\Session Manager\kernel",
                "GlobalTimerResolutionRequests",
                1,
            )]
        },
    },
    Tweak {
        id: "latency.usb_controller_power",
        name: "Disable USB controller power management",
        section: Section::Latency,
        summary: "Stops the USB host controllers entering low-power link states.",
        rationale: "Applies at the controller level rather than per device, so it covers \
                    everything plugged in including hubs. Link power management on a USB \
                    controller adds wake latency to every device behind it.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some("Higher idle power draw."),
        applies_to: always,
        build: |_| {
            vec![
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Services\USBXHCI\Parameters",
                    "EnableSelectiveSuspend",
                    0,
                ),
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Services\USBHUB3\Parameters",
                    "DisableSelectiveSuspend",
                    1,
                ),
            ]
        },
    },
    Tweak {
        id: "latency.disable_pcie_aspm",
        name: "Disable PCIe link power management",
        section: Section::Latency,
        summary: "Turns off Active State Power Management for PCI Express links.",
        rationale: "ASPM drops idle PCIe links into a low-power state. Bringing a link back up \
                    takes microseconds, and it happens on the path between the CPU and both your \
                    GPU and your network card.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("Higher idle power draw across the whole platform."),
        applies_to: |p| !p.power.is_laptop,
        build: |_| {
            vec![command(
                "powercfg.exe",
                &[
                    "/setacvalueindex",
                    "SCHEME_CURRENT",
                    "501a4d13-42af-4429-9fd1-a8218c268e20",
                    "ee12f906-d277-404b-b6da-e5fa1a576df5",
                    "0",
                ],
                &[
                    "/setacvalueindex",
                    "SCHEME_CURRENT",
                    "501a4d13-42af-4429-9fd1-a8218c268e20",
                    "ee12f906-d277-404b-b6da-e5fa1a576df5",
                    "2",
                ],
            )]
        },
    },
    Tweak {
        id: "latency.disable_hpet_device",
        name: "Leave the platform timer selection to Windows",
        section: Section::Latency,
        summary: "Ensures no boot flag is forcing a specific timer source.",
        rationale: "Both forcing HPET on and disabling it in Device Manager are popular advice, \
                    and both are wrong on current hardware. Windows picks the invariant TSC when \
                    available, which is faster to read than any alternative. The right action is \
                    to remove overrides, which this does.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![Action::RunCommand {
                program: "bcdedit.exe".into(),
                args: vec!["/deletevalue".into(), "tscsyncpolicy".into()],
                revert: RevertCommand::new("bcdedit.exe", &["/deletevalue", "tscsyncpolicy"]),
                tolerate_exit_codes: vec![1, -1],
            }]
        },
    },
    Tweak {
        id: "latency.irq8_priority",
        name: "System CMOS interrupt priority",
        section: Section::Latency,
        summary: "Sets IRQ8Priority on the real-time clock.",
        rationale: "A staple of every latency guide since Windows XP. The value applied to the \
                    legacy PIC interrupt line for the CMOS clock, which modern systems do not \
                    route through the PIC at all. Included because people check for it; it does \
                    nothing on any machine that can run Windows 11.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::NoMeasuredBenefit,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\PriorityControl",
                "IRQ8Priority",
                1,
            )]
        },
    },
    Tweak {
        id: "latency.disable_dpc_watchdog_timeout",
        name: "Relax the DPC watchdog timeout",
        section: Section::Latency,
        summary: "Raises the threshold before Windows bugchecks on a long-running DPC.",
        rationale: "Not a performance change. Prevents a DPC_WATCHDOG_VIOLATION blue screen when \
                    a driver takes an unusually long time under heavy load — most often seen with \
                    storage controllers during shader compilation.",
        risk: Risk::Medium,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some(
            "A genuinely broken driver will hang the machine for longer before the \
                        watchdog catches it.",
        ),
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\Session Manager\Kernel",
                "DpcWatchdogProfileOffset",
                0,
            )]
        },
    },
];
