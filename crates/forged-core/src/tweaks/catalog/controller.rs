//! Controller.
//!
//! Gamepad latency on Windows is dominated by three things: the transport
//! (wired beats 2.4 GHz beats Bluetooth by a wide margin), USB power management
//! idling the pad between reports, and the Game Bar overlay hooking the Guide
//! button. Only the last two are software-fixable, and this section fixes both.
//!
//! Note the deliberate *non*-tweak: `XboxGipSvc` is the Xbox accessory driver
//! service. Generic "debloat" scripts disable it, which breaks wired Xbox pads
//! outright. Forged detects a connected controller and explicitly ensures the
//! service stays enabled.

use crate::hardware::{ControllerConnection, HardwareProfile};
use crate::tweaks::model::*;

/// Clears USB power management on each detected gamepad.
fn controller_power_actions(profile: &HardwareProfile) -> Vec<Action> {
    let mut actions = Vec::new();
    for pad in &profile.peripherals.controllers {
        if pad.pnp_device_id.is_empty() {
            continue;
        }
        let path = format!(
            r"SYSTEM\CurrentControlSet\Enum\{}\Device Parameters",
            pad.pnp_device_id
        );
        actions.push(hklm_dword(&path, "EnhancedPowerManagementEnabled", 0));
        actions.push(hklm_dword(&path, "SelectiveSuspendEnabled", 0));
        actions.push(hklm_dword(&path, "AllowIdleIrpInD3", 0));
    }
    actions
}

/// Whether any detected controller uses a wireless transport.
fn has_wireless_pad(profile: &HardwareProfile) -> bool {
    profile.peripherals.controllers.iter().any(|c| {
        matches!(
            c.connection,
            ControllerConnection::Bluetooth | ControllerConnection::ProprietaryWireless
        )
    })
}

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "controller.usb_power_management",
        name: "Stop Windows idling your controller",
        section: Section::Controller,
        summary: "Clears selective suspend and enhanced power management on every detected \
                  gamepad.",
        rationale: "Windows idles a quiet USB device into a low-power state. On a controller \
                    this shows up as the first stick input after a still moment arriving late — \
                    the classic 'my first shot after landing feels dead' complaint.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| !p.peripherals.controllers.is_empty(),
        build: controller_power_actions,
    },
    Tweak {
        id: "controller.keep_xbox_accessory_service",
        name: "Protect the Xbox accessory driver service",
        section: Section::Controller,
        summary: "Ensures XboxGipSvc and the HID gamepad services are enabled rather than \
                  disabled.",
        rationale: "Almost every 'debloat' script on the internet disables the Xbox services \
                    wholesale, which stops wired Xbox controllers from being recognised at all. \
                    Forged deliberately re-enables them when a pad is present. This entry exists \
                    to undo damage other tools do.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| !p.peripherals.controllers.is_empty(),
        build: |_| {
            vec![
                service("XboxGipSvc", ServiceStart::Manual),
                service("HidService", ServiceStart::Manual),
                service("BthHFSrv", ServiceStart::Manual),
            ]
        },
    },
    Tweak {
        id: "controller.game_bar_guide_button",
        name: "Unbind the Guide button from Game Bar",
        section: Section::Controller,
        summary: "Stops the Xbox/PlayStation button from opening the Game Bar overlay.",
        rationale: "The overlay hooks the render loop to draw itself. Opening it mid-match costs \
                    a visible hitch, and an accidental press during a build fight is a lost fight. \
                    The button still works for the console's own functions.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_dword(r"Software\Microsoft\GameBar", "UseNexusForGameBarEnabled", 0),
                hkcu_dword(r"Software\Microsoft\GameBar", "ShowStartupPanel", 0),
                hkcu_dword(r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "AppCaptureEnabled", 0),
            ]
        },
    },
    Tweak {
        id: "controller.bluetooth_radio_power",
        name: "Disable Bluetooth radio power saving",
        section: Section::Controller,
        summary: "Prevents Windows from powering down the Bluetooth radio while a pad is paired.",
        rationale: "A Bluetooth controller that stops reporting for a moment forces a link \
                    renegotiation, which is felt as a brief total input dropout. Keeping the \
                    radio awake removes that failure mode, though it does not remove Bluetooth's \
                    baseline latency.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: has_wireless_pad,
        build: |_| {
            vec![
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters",
                    "IdleTimeoutEnabled",
                    0,
                ),
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Services\BTHUSB\Parameters",
                    "IdleTimeout",
                    0,
                ),
            ]
        },
    },
    Tweak {
        id: "controller.xinput_polling_priority",
        name: "Raise XInput service responsiveness",
        section: Section::Controller,
        summary: "Marks the gamepad input service as a latency-sensitive multimedia task.",
        rationale: "Places controller polling in the same scheduling category as audio, so it is \
                    not preempted by background work during a loaded frame.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| !p.peripherals.controllers.is_empty(),
        build: |_| {
            let path = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games";
            vec![
                hklm_sz(path, "Scheduling Category", "High"),
                hklm_sz(path, "SFIO Priority", "High"),
                hklm_dword(path, "Priority", 6),
            ]
        },
    },
    Tweak {
        id: "controller.disable_joystick_calibration",
        name: "Clear stale stick calibration data",
        section: Section::Controller,
        summary: "Removes saved joystick calibration curves from previous devices.",
        rationale: "Windows keeps calibration per VID/PID, and a stale entry from a worn-out pad \
                    applies its drift compensation to a new controller with the same identifiers. \
                    Clearing it makes the pad report raw values, which is what the game wants.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::SituationalGain,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| !p.peripherals.controllers.is_empty(),
        build: |_| {
            vec![Action::DeleteRegistryValue {
                hive: Hive::CurrentUser,
                path: r"System\CurrentControlSet\Control\MediaProperties\PrivateProperties\Joystick\Winmm".into(),
                value: "PIDVIDCalibration".into(),
            }]
        },
    },
    Tweak {
        id: "controller.hid_selective_suspend_global",
        name: "Disable HID selective suspend at the class level",
        section: Section::Controller,
        summary: "Turns off idle suspend for the whole human-interface-device class.",
        rationale: "Catches gamepads connected through hubs, wireless dongles and docks, whose \
                    device nodes change identity between reboots and so cannot be targeted \
                    individually.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Services\HidUsb\Parameters",
                    "SelectiveSuspendEnabled",
                    0,
                ),
                hklm_dword(
                    r"SYSTEM\CurrentControlSet\Services\usbhub\Parameters",
                    "DisableSelectiveSuspend",
                    1,
                ),
            ]
        },
    },
    Tweak {
        id: "controller.disable_gamepad_text_input",
        name: "Disable the gamepad touch keyboard popup",
        section: Section::Controller,
        summary: "Stops the on-screen keyboard appearing when a text field is focused with a pad \
                  connected.",
        rationale: "The popup grabs focus and can minimise a fullscreen game. Harmless when it \
                    does not fire and match-losing when it does.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| !p.peripherals.controllers.is_empty(),
        build: |_| {
            vec![hkcu_dword(
                r"SOFTWARE\Microsoft\TabletTip\1.7",
                "EnableGamepadTextInput",
                0,
            )]
        },
    },
    Tweak {
        id: "controller.wireless_adapter_power",
        name: "Keep the wireless controller dongle awake",
        section: Section::Controller,
        summary: "Disables power management on Xbox Wireless Adapter and proprietary 2.4 GHz \
                  receivers.",
        rationale: "A suspended dongle drops the link entirely and takes a second or more to \
                    re-establish. On a dedicated gaming machine there is no reason to ever let \
                    the receiver sleep.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: has_wireless_pad,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Services\xboxgip\Parameters",
                "SelectiveSuspendEnabled",
                0,
            )]
        },
    },
    Tweak {
        id: "controller.remove_deadzone_override",
        name: "Clear third-party deadzone overrides",
        section: Section::Controller,
        summary: "Removes DirectInput deadzone and saturation overrides written by mapping tools.",
        rationale: "DS4Windows, x360ce and similar utilities leave deadzone values in the registry \
                    after uninstall, which stack on top of the game's own deadzone and make small \
                    stick inputs vanish. Fortnite handles its own deadzone in-game.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| !p.peripherals.controllers.is_empty(),
        build: |_| {
            let path = r"System\CurrentControlSet\Control\MediaProperties\PrivateProperties\DirectInput";
            vec![
                Action::DeleteRegistryValue {
                    hive: Hive::CurrentUser,
                    path: path.into(),
                    value: "DeadZone".into(),
                },
                Action::DeleteRegistryValue {
                    hive: Hive::CurrentUser,
                    path: path.into(),
                    value: "Saturation".into(),
                },
            ]
        },
    },
];
