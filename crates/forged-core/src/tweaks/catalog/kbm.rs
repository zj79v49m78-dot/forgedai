//! Keyboard & mouse.
//!
//! This section is about *input fidelity*, not frame rate. The changes here
//! remove software layers that sit between the sensor and the game: pointer
//! acceleration, the accessibility input filters, and USB power management that
//! can idle a device between reports.

use crate::hardware::HardwareProfile;
use crate::tweaks::model::*;

/// The linear response curve Windows uses when acceleration is off.
///
/// `SmoothMouseXCurve` and `SmoothMouseYCurve` are 5-point fixed-point curves.
/// Even with "Enhance pointer precision" unchecked, a previously-modified curve
/// stays in the registry and keeps applying, so a clean machine and a
/// "optimised-by-a-YouTube-script" machine behave differently. These are the
/// stock linear values.
const LINEAR_X_CURVE: [u8; 40] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0xC0, 0xCC, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x80, 0x99, 0x19, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x40, 0x66, 0x26, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x33, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00,
];

const LINEAR_Y_CURVE: [u8; 40] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0xA8, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Per-device power management for HID hardware.
///
/// Walks the detected mice and keyboards and clears selective suspend on each
/// device's `Device Parameters` node. Devices are addressed by their real PnP
/// instance path from the scan, so this cannot touch unrelated hardware.
fn hid_power_actions(profile: &HardwareProfile) -> Vec<Action> {
    let mut actions = Vec::new();
    let devices = profile
        .peripherals
        .mice
        .iter()
        .chain(profile.peripherals.keyboards.iter());

    for device in devices {
        if device.pnp_device_id.is_empty() {
            continue;
        }
        let path = format!(
            r"SYSTEM\CurrentControlSet\Enum\{}\Device Parameters",
            device.pnp_device_id
        );
        actions.push(hklm_dword(&path, "EnhancedPowerManagementEnabled", 0));
        actions.push(hklm_dword(&path, "SelectiveSuspendEnabled", 0));
        actions.push(hklm_dword(&path, "AllowIdleIrpInD3", 0));
    }
    actions
}

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "kbm.pointer_acceleration",
        name: "Disable mouse acceleration",
        section: Section::KeyboardMouse,
        summary: "Turns off 'Enhance pointer precision' and clears the acceleration thresholds \
                  so cursor travel is a fixed multiple of sensor movement.",
        rationale: "With acceleration on, the same physical flick produces a different in-game \
                    turn depending on how fast you moved. Muscle memory cannot form against a \
                    moving target. This is the single most important input setting on the machine.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_sz(r"Control Panel\Mouse", "MouseSpeed", "0"),
                hkcu_sz(r"Control Panel\Mouse", "MouseThreshold1", "0"),
                hkcu_sz(r"Control Panel\Mouse", "MouseThreshold2", "0"),
            ]
        },
    },
    Tweak {
        id: "kbm.pointer_curves",
        name: "Reset pointer response curves to linear",
        section: Section::KeyboardMouse,
        summary: "Restores SmoothMouseXCurve and SmoothMouseYCurve to their stock linear values.",
        rationale: "These curves keep applying even when acceleration is unchecked in the \
                    control panel. A machine that has had a tweaking script run on it can carry \
                    a modified curve indefinitely; resetting them guarantees a true 1:1 baseline.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                Action::SetRegistry {
                    hive: Hive::CurrentUser,
                    path: r"Control Panel\Mouse".into(),
                    value: "SmoothMouseXCurve".into(),
                    data: RegData::Binary(LINEAR_X_CURVE.to_vec()),
                },
                Action::SetRegistry {
                    hive: Hive::CurrentUser,
                    path: r"Control Panel\Mouse".into(),
                    value: "SmoothMouseYCurve".into(),
                    data: RegData::Binary(LINEAR_Y_CURVE.to_vec()),
                },
            ]
        },
    },
    Tweak {
        id: "kbm.pointer_speed_neutral",
        name: "Set pointer speed to the neutral notch",
        section: Section::KeyboardMouse,
        summary: "Sets the Windows pointer speed slider to 6/11, the only position that applies \
                  no scaling to mouse input.",
        rationale: "Every slider position except the middle one multiplies or divides raw counts, \
                    which quantises small movements and costs precision on micro-adjustments. \
                    Sensitivity belongs in the game and in the mouse's own DPI, not in Windows.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| vec![hkcu_sz(r"Control Panel\Mouse", "MouseSensitivity", "10")],
    },
    Tweak {
        id: "kbm.accessibility_filters",
        name: "Disable Sticky, Filter and Toggle Keys",
        section: Section::KeyboardMouse,
        summary: "Switches off the three accessibility features that intercept keystrokes, \
                  including their pop-up prompts.",
        rationale: "Filter Keys deliberately delays and de-bounces key presses — it is an input \
                    delay feature by design. The prompts also steal focus mid-match, which on \
                    Fortnite means holding shift to sprint can drop you to desktop.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_sz(r"Control Panel\Accessibility\StickyKeys", "Flags", "506"),
                hkcu_sz(r"Control Panel\Accessibility\Keyboard Response", "Flags", "122"),
                hkcu_sz(r"Control Panel\Accessibility\ToggleKeys", "Flags", "58"),
                hkcu_sz(r"Control Panel\Accessibility\MouseKeys", "Flags", "58"),
            ]
        },
    },
    Tweak {
        id: "kbm.keyboard_repeat",
        name: "Fastest keyboard repeat rate",
        section: Section::KeyboardMouse,
        summary: "Sets repeat delay to the shortest setting and repeat rate to the fastest.",
        rationale: "Affects held-key behaviour in menus, chat and the building editor. Costs \
                    nothing and removes a small, constant source of sluggishness.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_sz(r"Control Panel\Keyboard", "KeyboardDelay", "0"),
                hkcu_sz(r"Control Panel\Keyboard", "KeyboardSpeed", "31"),
            ]
        },
    },
    Tweak {
        id: "kbm.hid_power_management",
        name: "Stop Windows idling your mouse and keyboard",
        section: Section::KeyboardMouse,
        summary: "Clears selective suspend and enhanced power management on every detected \
                  mouse and keyboard.",
        rationale: "Windows can put an idle USB input device into a low-power state. Waking it \
                    costs several milliseconds on the first report — felt as a tiny stutter on \
                    the first movement after a still moment, exactly when you are lining up a shot.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| !p.peripherals.mice.is_empty() || !p.peripherals.keyboards.is_empty(),
        build: hid_power_actions,
    },
    Tweak {
        id: "kbm.usb_selective_suspend",
        name: "Disable USB selective suspend globally",
        section: Section::KeyboardMouse,
        summary: "Turns off the power plan's USB selective suspend setting on AC power.",
        rationale: "The per-device setting above covers known peripherals; this covers the hubs \
                    and controllers they sit behind, which is where the wake latency actually \
                    accrues.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![command(
                "powercfg.exe",
                &[
                    "/setacvalueindex",
                    "SCHEME_CURRENT",
                    "2a737441-1930-4402-8d77-b2bebba308a3",
                    "48e6b7a6-50f5-4782-a5d4-53bb8f07e226",
                    "0",
                ],
                &[
                    "/setacvalueindex",
                    "SCHEME_CURRENT",
                    "2a737441-1930-4402-8d77-b2bebba308a3",
                    "48e6b7a6-50f5-4782-a5d4-53bb8f07e226",
                    "1",
                ],
            )]
        },
    },
    Tweak {
        id: "kbm.pointer_trails",
        name: "Disable pointer trails and shadow",
        section: Section::KeyboardMouse,
        summary: "Removes the cursor trail and drop shadow compositing effects.",
        rationale: "Both are drawn by the desktop compositor on every cursor move. The cost is \
                    tiny but it is paid on the exact code path that draws your crosshair in menus.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_sz(r"Control Panel\Mouse", "MouseTrails", "0"),
                hkcu_sz(r"Control Panel\Desktop", "UserPreferencesMask", "9012038010000000"),
            ]
        },
    },
    Tweak {
        id: "kbm.mouse_data_queue_size",
        name: "Mouse and keyboard buffer size",
        section: Section::KeyboardMouse,
        summary: "Sets MouseDataQueueSize and KeyboardDataQueueSize to their default of 100 \
                  packets.",
        rationale: "Widely recommended in tweaking guides, usually with a claim that raising or \
                    lowering it reduces input lag. It does not: the buffer only matters if reports \
                    arrive faster than the class driver drains them, which does not happen at any \
                    real polling rate. Included at the stock value so a machine that has had a \
                    tweaking script run on it is put back to sane, and reported as neutral.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::NoMeasuredBenefit,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Services\mouclass\Parameters",
                    "MouseDataQueueSize",
                    100,
                ),
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Services\kbdclass\Parameters",
                    "KeyboardDataQueueSize",
                    100,
                ),
            ]
        },
    },
    Tweak {
        id: "kbm.snap_to_default_button",
        name: "Disable snap-to on dialog buttons",
        section: Section::KeyboardMouse,
        summary: "Stops Windows from teleporting the cursor onto the default button of dialogs.",
        rationale: "An unexpected cursor jump while alt-tabbed out of a match is disorienting and \
                    can drop a click on the wrong control. No performance effect; pure predictability.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| vec![hkcu_sz(r"Control Panel\Mouse", "SnapToDefaultButton", "0")],
    },
    Tweak {
        id: "kbm.legacy_mouse_polling",
        name: "Remove legacy mouse polling overrides",
        section: Section::KeyboardMouse,
        summary: "Deletes third-party polling-rate overrides left in the mouclass and i8042 \
                  driver keys.",
        rationale: "Old overclocking utilities write persistent values here that stay after the \
                    tool is uninstalled, and can leave a modern USB mouse polling at a rate its \
                    firmware never intended. Removing them returns control to the device.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                Action::DeleteRegistryValue {
                    hive: Hive::LocalMachine,
                    path: r"SYSTEM\CurrentControlSet\Services\mouclass\Parameters".into(),
                    value: "MouseSynchIn100ns".into(),
                },
                Action::DeleteRegistryValue {
                    hive: Hive::LocalMachine,
                    path: r"SYSTEM\CurrentControlSet\Services\i8042prt\Parameters".into(),
                    value: "PollStatusIterations".into(),
                },
            ]
        },
    },
];
