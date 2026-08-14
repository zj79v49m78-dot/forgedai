//! Elevation check and System Restore integration.
//!
//! Forged refuses to apply anything without an elevated token, and refuses to
//! apply anything without first attempting a restore point. The restore point is
//! belt-and-braces: the journal is the primary undo mechanism and is far more
//! precise, but a restore point covers the case where the machine will not boot
//! far enough to run Forged again.

use crate::error::{ForgedError, Result};
use crate::win::process;

/// Whether the current process holds an elevated administrator token.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size,
            &mut size,
        )
        .is_ok();

        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Hard gate used at the top of every apply run.
pub fn require_elevation() -> Result<()> {
    if is_elevated() {
        Ok(())
    } else {
        Err(ForgedError::NotElevated(
            "Forged must be run as Administrator. Right-click the shortcut and choose \
             'Run as administrator', or reinstall so the elevation manifest is registered."
                .into(),
        ))
    }
}

/// Ensures System Restore is turned on for the system drive.
///
/// On a fresh Windows 11 install, protection is frequently disabled by default,
/// in which case `Checkpoint-Computer` silently does nothing. Enabling it first
/// is the difference between having a fallback and believing you have one.
pub fn enable_system_restore() -> Result<()> {
    let script = "Enable-ComputerRestore -Drive $env:SystemDrive -ErrorAction Stop";
    process::powershell(script).map(|_| ())
}

/// Creates a named restore point.
///
/// Windows rate-limits restore points to one per 24 hours by default; this
/// clears `SystemRestorePointCreationFrequency` first so a Forged run always
/// gets its own checkpoint.
pub fn create_restore_point(description: &str) -> Result<()> {
    // Remove the 24-hour throttle so consecutive runs each get a checkpoint.
    let _ = crate::win::registry::set_value(
        crate::tweaks::model::Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore",
        "SystemRestorePointCreationFrequency",
        &crate::tweaks::model::RegData::Dword(0),
    );

    let escaped = description.replace('"', "'");
    let script = format!(
        "Checkpoint-Computer -Description \"{escaped}\" \
         -RestorePointType MODIFY_SETTINGS -ErrorAction Stop"
    );
    process::powershell(&script).map(|_| ())
}

/// Best-effort restore point creation that reports failure without aborting.
///
/// Some editions and some OEM images have System Restore genuinely unavailable.
/// The journal still provides full undo, so the run proceeds — but the UI states
/// plainly that the firmware-level fallback is missing.
pub fn try_create_restore_point(description: &str) -> RestorePointResult {
    if let Err(e) = enable_system_restore() {
        return RestorePointResult::Unavailable(format!("could not enable System Restore: {e}"));
    }
    match create_restore_point(description) {
        Ok(()) => RestorePointResult::Created,
        Err(e) => RestorePointResult::Unavailable(e.to_string()),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RestorePointResult {
    Created,
    Unavailable(String),
}

impl RestorePointResult {
    pub fn created(&self) -> bool {
        matches!(self, RestorePointResult::Created)
    }
}
