//! Thin, auditable wrappers over the Windows surfaces Forged touches.
//!
//! Nothing above this module calls `windows-sys`, spawns a process, or writes to
//! the registry directly. Keeping the OS boundary in one place is what makes the
//! engine reviewable: every mutation the app can perform is visible in the four
//! files here.

pub mod elevation;
pub mod process;
pub mod registry;
pub mod service;
