//! # Forged core engine
//!
//! Hardware scan → AI-selected plan → reversible apply → report.
//!
//! ## Design constraints
//!
//! Three rules shape everything in this crate:
//!
//! 1. **The AI selects, it never authors.** The planner receives catalog
//!    *metadata* and returns catalog *IDs*. Every returned ID is validated
//!    against the catalog before execution, so a hallucinated registry path is
//!    structurally impossible rather than merely unlikely.
//!
//! 2. **Nothing is applied that cannot be un-applied.** Registry and service
//!    changes capture their prior state at write time; command actions must
//!    declare an inverse or the engine refuses them. The journal is written
//!    before a change is reported as successful.
//!
//! 3. **Nothing touches the game.** Forged tunes the operating system around
//!    Fortnite and never reads, writes, injects into, or hooks the game process.
//!    Easy Anti-Cheat treats process interference as a ban condition, and no
//!    frame rate is worth an account.

pub mod ai;
pub mod bios;
pub mod error;
pub mod hardware;
pub mod journal;
pub mod report;
pub mod scan;
pub mod secure;
pub mod tweaks;
pub mod win;

pub use error::{ForgedError, Result, TweakFailure};
pub use hardware::HardwareProfile;
pub use tweaks::{Section, Tweak};

/// Application version, surfaced in the UI and stamped into every journal.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Journal format version. Bumped only on breaking changes to the on-disk shape,
/// so a newer Forged can still revert an older run.
pub const JOURNAL_SCHEMA: u32 = 1;
