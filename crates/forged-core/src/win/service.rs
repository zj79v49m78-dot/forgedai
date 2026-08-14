//! Service start-type control.
//!
//! Deliberately implemented through the registry rather than the Service Control
//! Manager. `Start` under `HKLM\SYSTEM\CurrentControlSet\Services\<name>` is the
//! authoritative value SCM itself reads at boot, and routing through the registry
//! means service changes journal and revert through exactly the same code path as
//! every other change — one format, one restore routine, no special cases.

use crate::error::{ForgedError, Result};
use crate::tweaks::model::{Hive, RegData, ServiceStart};
use crate::win::registry;

const SERVICES_ROOT: &str = r"SYSTEM\CurrentControlSet\Services";

fn service_path(name: &str) -> String {
    format!(r"{SERVICES_ROOT}\{name}")
}

/// Whether the service is present on this installation.
///
/// Many entries in the catalog target services that only exist on some SKUs or
/// after certain feature installs. Absence is not an error — the engine skips.
pub fn exists(name: &str) -> Result<bool> {
    Ok(registry::get_value(Hive::LocalMachine, &service_path(name), "Start")?.is_some())
}

pub fn get_start(name: &str) -> Result<Option<ServiceStart>> {
    match registry::get_value(Hive::LocalMachine, &service_path(name), "Start")? {
        Some(RegData::Dword(v)) => Ok(ServiceStart::from_u32(v)),
        Some(_) => Err(ForgedError::Service {
            service: name.to_string(),
            detail: "Start value is not a DWORD".into(),
        }),
        None => Ok(None),
    }
}

/// Sets the start type, returning the prior value for the journal.
///
/// Returns `Ok(None)` when the service does not exist, which the engine records
/// as a skip rather than a failure.
pub fn set_start(name: &str, start: ServiceStart) -> Result<Option<ServiceStart>> {
    let previous = get_start(name)?;
    if previous.is_none() {
        return Ok(None);
    }

    registry::set_value(
        Hive::LocalMachine,
        &service_path(name),
        "Start",
        &RegData::Dword(start as u32),
    )
    .map_err(|e| ForgedError::Service {
        service: name.to_string(),
        detail: e.to_string(),
    })?;

    Ok(previous)
}

/// Stops a running service. Best-effort: a service that refuses to stop is not
/// a failure, because the start-type change still takes effect at next boot.
pub fn try_stop(name: &str) -> Result<bool> {
    let out = crate::win::process::run("sc.exe", &["stop", name])?;
    Ok(out.status == 0)
}
