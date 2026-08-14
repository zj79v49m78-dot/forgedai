//! Error taxonomy for the Forged engine.
//!
//! Every fallible operation in the engine funnels through [`ForgedError`]. The
//! variants are deliberately coarse: the UI only ever needs to know *which
//! stage* failed and whether the failure is recoverable, and the detail string
//! carries the rest.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, ForgedError>;

#[derive(Debug, thiserror::Error)]
pub enum ForgedError {
    /// Forged is not running with an elevated token. Almost every actuator
    /// needs this, so it is checked once at startup rather than per-tweak.
    #[error("administrator privileges are required: {0}")]
    NotElevated(String),

    /// A registry read/write/delete failed.
    #[error("registry operation failed at {path}\\{value}: {source_msg}")]
    Registry {
        path: String,
        value: String,
        source_msg: String,
    },

    /// A Windows service could not be queried or reconfigured.
    #[error("service '{service}' operation failed: {detail}")]
    Service { service: String, detail: String },

    /// An external process (powercfg, netsh, bcdedit, PowerShell) failed.
    #[error("command `{command}` exited with {code}: {stderr}")]
    Command {
        command: String,
        code: i32,
        stderr: String,
    },

    /// The hardware scan could not complete.
    #[error("hardware scan failed during {stage}: {detail}")]
    Scan { stage: String, detail: String },

    /// The Claude API call failed, or returned something unusable.
    #[error("AI planner failed: {0}")]
    Ai(String),

    /// The API key is missing, malformed, or could not be decrypted.
    #[error("API key problem: {0}")]
    ApiKey(String),

    /// The rollback journal is missing, corrupt, or could not be written.
    #[error("rollback journal error: {0}")]
    Journal(String),

    /// A tweak referenced an ID that does not exist in the catalog. This is the
    /// backstop that makes AI-selected plans safe.
    #[error("unknown tweak id '{0}' (not present in catalog)")]
    UnknownTweak(String),

    /// This platform is not Windows. The engine compiles everywhere so it can be
    /// unit-tested on CI, but actuators refuse to run off-Windows.
    #[error("unsupported platform: Forged requires Windows 10 1809 or newer")]
    UnsupportedPlatform,

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialisation error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl ForgedError {
    /// Whether the operation can be retried without operator intervention.
    ///
    /// The UI uses this to decide between "Retry" and "Fix and retry". Note that
    /// a failed tweak is never fatal to a run: the engine records it and moves
    /// on, because a partially applied plan that is fully journalled is far
    /// safer than aborting halfway with no record.
    pub fn is_transient(&self) -> bool {
        matches!(self, ForgedError::Ai(_) | ForgedError::Io(_))
    }

    /// Short stage label for telemetry-free local logging and UI grouping.
    pub fn stage(&self) -> &'static str {
        match self {
            ForgedError::NotElevated(_) => "elevation",
            ForgedError::Registry { .. } => "registry",
            ForgedError::Service { .. } => "service",
            ForgedError::Command { .. } => "command",
            ForgedError::Scan { .. } => "scan",
            ForgedError::Ai(_) => "ai",
            ForgedError::ApiKey(_) => "apikey",
            ForgedError::Journal(_) => "journal",
            ForgedError::UnknownTweak(_) => "catalog",
            ForgedError::UnsupportedPlatform => "platform",
            ForgedError::Io(_) => "io",
            ForgedError::Serde(_) => "serde",
        }
    }
}

/// Wrapper so a failed individual tweak can be reported without aborting the run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TweakFailure {
    pub tweak_id: String,
    pub stage: String,
    pub message: String,
}

impl fmt::Display for TweakFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.stage, self.tweak_id, self.message)
    }
}

impl TweakFailure {
    pub fn from_error(tweak_id: impl Into<String>, err: &ForgedError) -> Self {
        Self {
            tweak_id: tweak_id.into(),
            stage: err.stage().to_string(),
            message: err.to_string(),
        }
    }
}
