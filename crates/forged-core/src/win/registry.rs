//! Registry access with mandatory prior-state capture.
//!
//! The only write entry point is [`set_value`], and it returns the value that
//! was there before (or `None` if the value did not exist). The engine persists
//! that return value to the journal *before* considering the write successful,
//! which is what makes every registry change in Forged reversible.

use crate::error::{ForgedError, Result};
use crate::tweaks::model::{Hive, RegData};

#[cfg(windows)]
use windows::core::{PCWSTR, PWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS,
    KEY_READ, KEY_WOW64_64KEY, KEY_WRITE, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ,
    REG_OPTION_NON_VOLATILE, REG_QWORD, REG_SZ, REG_VALUE_TYPE,
};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn reg_err(path: &str, value: &str, msg: impl Into<String>) -> ForgedError {
    ForgedError::Registry {
        path: path.to_string(),
        value: value.to_string(),
        source_msg: msg.into(),
    }
}

#[cfg(windows)]
fn root(hive: Hive) -> HKEY {
    match hive {
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        Hive::CurrentUser => HKEY_CURRENT_USER,
        Hive::Users => HKEY_USERS,
        Hive::ClassesRoot => HKEY_CLASSES_ROOT,
    }
}

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------

#[cfg(windows)]
struct KeyHandle(HKEY);

#[cfg(windows)]
impl Drop for KeyHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
}

#[cfg(windows)]
fn open_read(hive: Hive, path: &str) -> Result<Option<KeyHandle>> {
    // Bound to a local so the buffer outlives the call unambiguously.
    let subkey = wide(path);
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            root(hive),
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut key,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(Some(KeyHandle(key))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        WIN32_ERROR(code) => Err(reg_err(path, "", format!("RegOpenKeyEx failed ({code})"))),
    }
}

/// Opens for write, creating the key (and any missing parents) if absent.
#[cfg(windows)]
fn open_or_create_write(hive: Hive, path: &str) -> Result<KeyHandle> {
    let subkey = wide(path);
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            root(hive),
            PCWSTR(subkey.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
            None,
            &mut key,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(reg_err(
            path,
            "",
            format!("RegCreateKeyEx failed ({})", status.0),
        ));
    }
    Ok(KeyHandle(key))
}

/// Reads a value, returning `None` if either the key or the value is absent.
#[cfg(windows)]
pub fn get_value(hive: Hive, path: &str, value: &str) -> Result<Option<RegData>> {
    let Some(key) = open_read(hive, path)? else {
        return Ok(None);
    };

    let name = wide(value);
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0u32;

    // First pass: ask for the required buffer size.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(reg_err(
            path,
            value,
            format!("RegQueryValueEx size probe failed ({})", status.0),
        ));
    }

    let mut buf = vec![0u8; size as usize];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(reg_err(
            path,
            value,
            format!("RegQueryValueEx read failed ({})", status.0),
        ));
    }
    buf.truncate(size as usize);

    Ok(Some(decode(kind, &buf)))
}

#[cfg(windows)]
fn decode(kind: REG_VALUE_TYPE, buf: &[u8]) -> RegData {
    match kind {
        REG_DWORD => {
            let mut b = [0u8; 4];
            let n = buf.len().min(4);
            b[..n].copy_from_slice(&buf[..n]);
            RegData::Dword(u32::from_ne_bytes(b))
        }
        REG_QWORD => {
            let mut b = [0u8; 8];
            let n = buf.len().min(8);
            b[..n].copy_from_slice(&buf[..n]);
            RegData::Qword(u64::from_ne_bytes(b))
        }
        REG_SZ => RegData::Sz(decode_wide(buf)),
        REG_EXPAND_SZ => RegData::ExpandSz(decode_wide(buf)),
        REG_MULTI_SZ => RegData::MultiSz(
            decode_wide(buf)
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
        ),
        _ => RegData::Binary(buf.to_vec()),
    }
}

/// Decodes a UTF-16LE byte buffer, trimming the trailing NUL terminator.
#[cfg(windows)]
fn decode_wide(buf: &[u8]) -> String {
    let units: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect();
    let trimmed = match units.split_last() {
        Some((0, rest)) => rest,
        _ => &units[..],
    };
    String::from_utf16_lossy(trimmed)
}

#[cfg(windows)]
fn encode(data: &RegData) -> (REG_VALUE_TYPE, Vec<u8>) {
    match data {
        RegData::Dword(v) => (REG_DWORD, v.to_ne_bytes().to_vec()),
        RegData::Qword(v) => (REG_QWORD, v.to_ne_bytes().to_vec()),
        RegData::Sz(s) => (REG_SZ, encode_wide(s)),
        RegData::ExpandSz(s) => (REG_EXPAND_SZ, encode_wide(s)),
        RegData::MultiSz(items) => {
            // MULTI_SZ is a run of NUL-terminated strings ending in a second NUL.
            let mut units: Vec<u16> = Vec::new();
            for item in items {
                units.extend(item.encode_utf16());
                units.push(0);
            }
            units.push(0);
            (REG_MULTI_SZ, units_to_bytes(&units))
        }
        RegData::Binary(b) => (REG_BINARY, b.clone()),
    }
}

#[cfg(windows)]
fn encode_wide(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    units_to_bytes(&units)
}

#[cfg(windows)]
fn units_to_bytes(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len() * 2);
    for u in units {
        out.extend_from_slice(&u.to_ne_bytes());
    }
    out
}

/// Writes a value and returns whatever was there before.
///
/// The returned `None` means "this value did not exist", which the journal
/// records so that reverting deletes the value rather than writing a guess.
#[cfg(windows)]
pub fn set_value(hive: Hive, path: &str, value: &str, data: &RegData) -> Result<Option<RegData>> {
    let previous = get_value(hive, path, value)?;

    let key = open_or_create_write(hive, path)?;
    let name = wide(value);
    let (kind, bytes) = encode(data);

    let status = unsafe {
        RegSetValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            0,
            kind,
            Some(bytes.as_slice()),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(reg_err(
            path,
            value,
            format!("RegSetValueEx failed ({})", status.0),
        ));
    }
    Ok(previous)
}

/// Deletes a value, returning its prior contents for the journal.
#[cfg(windows)]
pub fn delete_value(hive: Hive, path: &str, value: &str) -> Result<Option<RegData>> {
    let previous = get_value(hive, path, value)?;
    if previous.is_none() {
        return Ok(None); // already absent; nothing to journal
    }

    let key = open_or_create_write(hive, path)?;
    let name = wide(value);
    let status = unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) };
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        return Err(reg_err(
            path,
            value,
            format!("RegDeleteValue failed ({})", status.0),
        ));
    }
    Ok(previous)
}

/// Enumerates immediate subkey names. Used by the scanner to walk device class
/// keys, where the interesting node is an opaque four-digit index.
#[cfg(windows)]
pub fn subkeys(hive: Hive, path: &str) -> Result<Vec<String>> {
    use windows::Win32::System::Registry::RegEnumKeyExW;

    let Some(key) = open_read(hive, path)? else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let mut index = 0u32;
    loop {
        // 256 UTF-16 units is the documented maximum key-name length.
        let mut name = vec![0u16; 256];
        let mut len = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                PWSTR(name.as_mut_ptr()),
                &mut len,
                None,
                PWSTR::null(),
                None,
                None,
            )
        };
        if status != ERROR_SUCCESS {
            break; // ERROR_NO_MORE_ITEMS, or a genuine failure we treat as end-of-list
        }
        name.truncate(len as usize);
        out.push(String::from_utf16_lossy(&name));
        index += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Non-Windows stubs
//
// The crate builds on Linux so the catalog, planner, and journal logic stay
// unit-testable in CI. Every actuator refuses to run.
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
pub fn get_value(_hive: Hive, _path: &str, _value: &str) -> Result<Option<RegData>> {
    Err(ForgedError::UnsupportedPlatform)
}

#[cfg(not(windows))]
pub fn set_value(_hive: Hive, _path: &str, _value: &str, _data: &RegData) -> Result<Option<RegData>> {
    Err(ForgedError::UnsupportedPlatform)
}

#[cfg(not(windows))]
pub fn delete_value(_hive: Hive, _path: &str, _value: &str) -> Result<Option<RegData>> {
    Err(ForgedError::UnsupportedPlatform)
}

#[cfg(not(windows))]
pub fn subkeys(_hive: Hive, _path: &str) -> Result<Vec<String>> {
    Err(ForgedError::UnsupportedPlatform)
}
