//! API key storage.
//!
//! The key is encrypted with DPAPI under the current user's credentials and
//! written to `%APPDATA%\Forged\credentials.bin`. DPAPI binds the ciphertext to
//! the Windows user account, so copying the file to another machine or another
//! user profile yields nothing.
//!
//! ## Why the key is not compiled into the binary
//!
//! An API key embedded in a distributed executable is extractable with a hex
//! editor in about thirty seconds — obfuscation only changes how long it takes.
//! Whoever holds the key pays for every call made with it, so Forged asks each
//! user for their own. This is why the app has a settings screen instead of a
//! hard-coded constant.

use crate::error::{ForgedError, Result};
use std::path::PathBuf;

#[cfg(windows)]
use windows::Win32::Foundation::{LocalFree, HLOCAL};
#[cfg(windows)]
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

/// Additional entropy mixed into the DPAPI blob. Not a secret — it scopes the
/// ciphertext to this application so another program running as the same user
/// cannot trivially decrypt it by pointing DPAPI at the file.
#[cfg(windows)]
const ENTROPY: &[u8] = b"forged.optimiser.v1";

fn credentials_path() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("Forged")
        .join("credentials.bin")
}

// ---------------------------------------------------------------------------
// Windows: DPAPI
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn protect(plaintext: &[u8]) -> Result<Vec<u8>> {
    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();

        CryptProtectData(
            &mut input,
            None,
            Some(&mut entropy),
            None,
            None,
            0,
            &mut output,
        )
        .map_err(|e| ForgedError::ApiKey(format!("DPAPI encryption failed: {e}")))?;

        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut core::ffi::c_void));
        Ok(bytes)
    }
}

#[cfg(windows)]
fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: ciphertext.len() as u32,
            pbData: ciphertext.as_ptr() as *mut u8,
        };
        let mut entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();

        CryptUnprotectData(
            &mut input,
            None,
            Some(&mut entropy),
            None,
            None,
            0,
            &mut output,
        )
        .map_err(|_| {
            ForgedError::ApiKey(
                "stored key could not be decrypted — it was saved by a different Windows user \
                 account. Enter your key again."
                    .into(),
            )
        })?;

        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut core::ffi::c_void));
        Ok(bytes)
    }
}

// ---------------------------------------------------------------------------
// Non-Windows: refuse rather than store plaintext
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn protect(_plaintext: &[u8]) -> Result<Vec<u8>> {
    Err(ForgedError::UnsupportedPlatform)
}

#[cfg(not(windows))]
fn unprotect(_ciphertext: &[u8]) -> Result<Vec<u8>> {
    Err(ForgedError::UnsupportedPlatform)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validates and stores an API key.
pub fn store_api_key(key: &str) -> Result<()> {
    let key = key.trim();
    validate_key_shape(key)?;

    let path = credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, protect(key.as_bytes())?)?;
    Ok(())
}

/// Loads the stored key, preferring the `ANTHROPIC_API_KEY` environment
/// variable when set so CI and power users can avoid the settings screen.
pub fn load_api_key() -> Result<String> {
    if let Ok(from_env) = std::env::var("ANTHROPIC_API_KEY") {
        let trimmed = from_env.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    let path = credentials_path();
    if !path.exists() {
        return Err(ForgedError::ApiKey(
            "no API key saved. Add one in Settings — Forged needs it to plan the optimisation."
                .into(),
        ));
    }

    let ciphertext = std::fs::read(&path)?;
    let plaintext = unprotect(&ciphertext)?;
    String::from_utf8(plaintext)
        .map_err(|_| ForgedError::ApiKey("stored key is not valid text".into()))
}

pub fn has_api_key() -> bool {
    load_api_key().is_ok()
}

/// Removes the stored key.
pub fn clear_api_key() -> Result<()> {
    let path = credentials_path();
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Cheap shape check so an obviously wrong paste is caught before a network
/// round trip. Deliberately loose — it is not this function's job to predict
/// future key formats.
fn validate_key_shape(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(ForgedError::ApiKey("the key is empty".into()));
    }
    if key.len() < 20 {
        return Err(ForgedError::ApiKey(
            "that looks too short to be an API key".into(),
        ));
    }
    if key.chars().any(|c| c.is_whitespace()) {
        return Err(ForgedError::ApiKey(
            "the key contains a space or newline — check for a copy/paste error".into(),
        ));
    }
    Ok(())
}

/// Masked form for display, e.g. `sk-ant-…J8fA`.
pub fn masked(key: &str) -> String {
    if key.len() <= 12 {
        return "•".repeat(key.len());
    }
    format!("{}…{}", &key[..7], &key[key.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_short_keys() {
        assert!(validate_key_shape("").is_err());
        assert!(validate_key_shape("sk-ant-short").is_err());
    }

    #[test]
    fn rejects_keys_containing_whitespace() {
        assert!(validate_key_shape("sk-ant-api03-abcdefghij klmnopqrstuv").is_err());
    }

    #[test]
    fn accepts_a_plausible_key() {
        assert!(validate_key_shape("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123").is_ok());
    }

    #[test]
    fn masking_hides_the_middle() {
        let masked = masked("sk-ant-api03-abcdefghijklmnop");
        assert!(masked.starts_with("sk-ant-"));
        assert!(masked.ends_with("mnop"));
        assert!(!masked.contains("abcdefghij"));
    }

    #[test]
    fn masking_a_short_string_reveals_nothing() {
        assert_eq!(masked("secret"), "••••••");
    }
}
