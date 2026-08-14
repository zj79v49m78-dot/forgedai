//! External command execution.
//!
//! A handful of Windows settings have no registry representation that survives
//! reboot correctly — power scheme values, TCP global parameters, boot
//! configuration — and must go through `powercfg`, `netsh`, or `bcdedit`. This
//! module is the only place Forged spawns a process.

use crate::error::{ForgedError, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn ok(&self) -> bool {
        self.status == 0
    }
}

/// Runs a command and captures its output without opening a console window.
pub fn run(program: &str, args: &[&str]) -> Result<CommandOutput> {
    run_owned(program, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

pub fn run_owned(program: &str, args: &[String]) -> Result<CommandOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args);

    // CREATE_NO_WINDOW. Without this, every powercfg call flashes a console.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let output = cmd.output().map_err(|e| ForgedError::Command {
        command: format!("{program} {}", args.join(" ")),
        code: -1,
        stderr: e.to_string(),
    })?;

    Ok(CommandOutput {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Runs a command, treating a listed exit code as success.
///
/// `netsh` and `powercfg` both return non-zero for "already in that state",
/// which is not a failure for our purposes.
pub fn run_tolerant(program: &str, args: &[String], tolerate: &[i32]) -> Result<CommandOutput> {
    let out = run_owned(program, args)?;
    if out.ok() || tolerate.contains(&out.status) {
        Ok(out)
    } else {
        Err(ForgedError::Command {
            command: format!("{program} {}", args.join(" ")),
            code: out.status,
            stderr: if out.stderr.trim().is_empty() {
                out.stdout.clone()
            } else {
                out.stderr.clone()
            },
        })
    }
}

/// Runs a PowerShell snippet and returns stdout.
///
/// Used exclusively for CIM/WMI inventory queries during the scan. `-NoProfile`
/// matters: a user profile script can add seconds and pollute stdout.
pub fn powershell(script: &str) -> Result<String> {
    let out = run(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
    )?;

    if !out.ok() {
        return Err(ForgedError::Command {
            command: format!("powershell -Command {script}"),
            code: out.status,
            stderr: out.stderr,
        });
    }
    Ok(out.stdout)
}

/// Runs a CIM query and parses the result as JSON.
///
/// `ConvertTo-Json` collapses single-element arrays into bare objects, so the
/// caller is handed a `Vec` either way by normalising here.
pub fn cim_query(class: &str, properties: &[&str]) -> Result<Vec<serde_json::Value>> {
    let props = properties.join(",");
    let script = format!(
        "Get-CimInstance -ClassName {class} -ErrorAction SilentlyContinue | \
         Select-Object {props} | ConvertTo-Json -Depth 4 -Compress"
    );
    let raw = powershell(&script)?;
    Ok(normalise_json_array(&raw))
}

/// Same as [`cim_query`] but against an arbitrary WMI namespace.
pub fn cim_query_ns(namespace: &str, class: &str, properties: &[&str]) -> Result<Vec<serde_json::Value>> {
    let props = properties.join(",");
    let script = format!(
        "Get-CimInstance -Namespace {namespace} -ClassName {class} -ErrorAction SilentlyContinue | \
         Select-Object {props} | ConvertTo-Json -Depth 4 -Compress"
    );
    let raw = powershell(&script)?;
    Ok(normalise_json_array(&raw))
}

/// Turns ConvertTo-Json output into a uniform array.
pub fn normalise_json_array(raw: &str) -> Vec<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Array(items)) => items,
        Ok(serde_json::Value::Null) => Vec::new(),
        Ok(other) => vec![other],
        Err(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Small extraction helpers used across the scanner
// ---------------------------------------------------------------------------

pub fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| {
            x.as_str()
                .map(|s| s.to_string())
                .or_else(|| x.as_i64().map(|n| n.to_string()))
        })
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn json_u64(v: &serde_json::Value, key: &str) -> u64 {
    v.get(key)
        .and_then(|x| {
            x.as_u64()
                .or_else(|| x.as_i64().map(|n| n.max(0) as u64))
                .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .unwrap_or(0)
}

pub fn json_u32(v: &serde_json::Value, key: &str) -> u32 {
    json_u64(v, key).min(u32::MAX as u64) as u32
}

pub fn json_bool(v: &serde_json::Value, key: &str) -> bool {
    v.get(key)
        .and_then(|x| x.as_bool().or_else(|| x.as_i64().map(|n| n != 0)))
        .unwrap_or(false)
}
