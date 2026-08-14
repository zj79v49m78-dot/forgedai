//! The rollback journal.
//!
//! Every mutation Forged performs is recorded here *before* it is reported as
//! successful, together with the exact prior state needed to undo it. The
//! journal is the primary undo mechanism; the System Restore point created at
//! the start of a run is only a fallback for the case where Windows will not
//! boot far enough to run Forged again.
//!
//! Journals live in `%ProgramData%\Forged\journals\` so they survive user-profile
//! resets and are readable by an elevated repair run.

use crate::error::{ForgedError, Result};
use crate::tweaks::model::{Hive, RegData, Section, ServiceStart};
use crate::win::{elevation::RestorePointResult, process, registry, service};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// On-disk shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Journal {
    pub schema: u32,
    pub forged_version: String,
    pub run_id: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub machine_summary: String,
    pub restore_point: RestorePointResult,
    pub entries: Vec<JournalEntry>,
    /// Set once the user has reverted this run, so the UI can grey it out.
    pub reverted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub tweak_id: String,
    pub tweak_name: String,
    pub section: Section,
    pub status: EntryStatus,
    /// Ordered undo operations. Reverted in reverse.
    pub undo: Vec<UndoRecord>,
    /// Populated when `status` is `Failed`.
    pub error: Option<String>,
    pub applied_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryStatus {
    Applied,
    /// The machine was already in the desired state; nothing was written.
    AlreadyCorrect,
    /// Not applicable to this hardware, or the target did not exist.
    Skipped,
    Failed,
}

/// The precise inverse of one action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UndoRecord {
    /// `previous: None` means the value did not exist, so undo deletes it.
    Registry {
        hive: Hive,
        path: String,
        value: String,
        previous: Option<RegData>,
    },
    Service {
        name: String,
        previous: ServiceStart,
    },
    /// The declared inverse invocation for a command action.
    Command { program: String, args: Vec<String> },
}

impl UndoRecord {
    pub fn describe(&self) -> String {
        match self {
            UndoRecord::Registry {
                hive,
                path,
                value,
                previous,
            } => match previous {
                Some(d) => format!(
                    "restore {}\\{}\\{} to {}",
                    hive.short_name(),
                    path,
                    value,
                    d.describe()
                ),
                None => format!("remove {}\\{}\\{}", hive.short_name(), path, value),
            },
            UndoRecord::Service { name, previous } => {
                format!("restore service {name} to {previous:?}")
            }
            UndoRecord::Command { program, args } => format!("{} {}", program, args.join(" ")),
        }
    }

    /// Performs the undo.
    pub fn apply(&self) -> Result<()> {
        match self {
            UndoRecord::Registry {
                hive,
                path,
                value,
                previous,
            } => {
                match previous {
                    Some(data) => registry::set_value(*hive, path, value, data).map(|_| ())?,
                    // The value was absent before Forged ran; absence is the
                    // correct restored state.
                    None => registry::delete_value(*hive, path, value).map(|_| ())?,
                }
                Ok(())
            }
            UndoRecord::Service { name, previous } => {
                service::set_start(name, *previous).map(|_| ())
            }
            UndoRecord::Command { program, args } => process::run_owned(program, args).map(|_| ()),
        }
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl Journal {
    pub fn new(machine_summary: String, restore_point: RestorePointResult) -> Self {
        let now = chrono::Utc::now();
        Self {
            schema: crate::JOURNAL_SCHEMA,
            forged_version: crate::VERSION.to_string(),
            // Sortable, filename-safe, and unique to the second — a run cannot
            // start twice in the same second on one machine.
            run_id: now.format("%Y%m%d-%H%M%S").to_string(),
            started_at: now.to_rfc3339(),
            completed_at: None,
            machine_summary,
            restore_point,
            entries: Vec::new(),
            reverted_at: None,
        }
    }

    pub fn push(&mut self, entry: JournalEntry) {
        self.entries.push(entry);
    }

    pub fn finish(&mut self) {
        self.completed_at = Some(chrono::Utc::now().to_rfc3339());
    }

    pub fn applied_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.status == EntryStatus::Applied)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.status == EntryStatus::Failed)
            .count()
    }

    pub fn skipped_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Skipped | EntryStatus::AlreadyCorrect))
            .count()
    }

    pub fn is_reverted(&self) -> bool {
        self.reverted_at.is_some()
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// `%ProgramData%\Forged`, falling back to the executable's directory on the
/// rare system where ProgramData is not resolvable.
pub fn data_dir() -> PathBuf {
    std::env::var("ProgramData")
        .map(|p| PathBuf::from(p).join("Forged"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn journal_dir() -> PathBuf {
    data_dir().join("journals")
}

impl Journal {
    pub fn path(&self) -> PathBuf {
        journal_dir().join(format!("{}.json", self.run_id))
    }

    /// Writes the journal to disk atomically.
    ///
    /// Called after *every* entry, not just at the end of a run: a power loss
    /// mid-run must still leave a complete record of what was already changed.
    /// The write goes to a temp file and is renamed, so a crash during the write
    /// itself cannot truncate a good journal.
    pub fn save(&self) -> Result<()> {
        let dir = journal_dir();
        std::fs::create_dir_all(&dir)?;

        let final_path = self.path();
        let tmp_path = final_path.with_extension("json.tmp");

        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Journal> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| ForgedError::Journal(format!("{}: {e}", path.display())))?;
        let journal: Journal = serde_json::from_str(&raw)
            .map_err(|e| ForgedError::Journal(format!("{} is corrupt: {e}", path.display())))?;

        if journal.schema > crate::JOURNAL_SCHEMA {
            return Err(ForgedError::Journal(format!(
                "journal {} was written by a newer version of Forged (schema {} > {})",
                journal.run_id,
                journal.schema,
                crate::JOURNAL_SCHEMA
            )));
        }
        Ok(journal)
    }
}

/// All journals on disk, newest first.
pub fn list() -> Result<Vec<Journal>> {
    let dir = journal_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // A single corrupt journal must not hide the others.
        match Journal::load(&path) {
            Ok(j) => out.push(j),
            Err(e) => tracing::warn!("skipping unreadable journal {}: {e}", path.display()),
        }
    }

    out.sort_by(|a, b| b.run_id.cmp(&a.run_id));
    Ok(out)
}

/// The most recent run that has not already been reverted.
pub fn latest_revertible() -> Result<Option<Journal>> {
    Ok(list()?.into_iter().find(|j| !j.is_reverted()))
}

// ---------------------------------------------------------------------------
// Revert
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertReport {
    pub run_id: String,
    pub restored: usize,
    pub failed: Vec<String>,
    pub reboot_recommended: bool,
}

/// Undoes a run.
///
/// Entries are processed in reverse order so that dependent changes unwind in
/// the order they were made. A failure on one record never stops the rest: a
/// partial restore is strictly better than abandoning the remaining records,
/// and every failure is reported so the user knows precisely what is still
/// changed.
pub fn revert(journal: &mut Journal) -> Result<RevertReport> {
    crate::win::elevation::require_elevation()?;

    let mut restored = 0usize;
    let mut failed = Vec::new();

    for entry in journal.entries.iter().rev() {
        if entry.status != EntryStatus::Applied {
            continue;
        }
        for record in entry.undo.iter().rev() {
            match record.apply() {
                Ok(()) => restored += 1,
                Err(e) => failed.push(format!("{} — {}: {e}", entry.tweak_id, record.describe())),
            }
        }
    }

    journal.reverted_at = Some(chrono::Utc::now().to_rfc3339());
    journal.save()?;

    Ok(RevertReport {
        run_id: journal.run_id.clone(),
        restored,
        failed,
        // Anything that needed a reboot to take effect needs one to come back.
        reboot_recommended: restored > 0,
    })
}

/// Reverts the most recent un-reverted run.
pub fn revert_latest() -> Result<RevertReport> {
    let mut journal = latest_revertible()?
        .ok_or_else(|| ForgedError::Journal("no un-reverted run found to roll back".into()))?;
    revert(&mut journal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_record_for_absent_value_deletes() {
        let record = UndoRecord::Registry {
            hive: Hive::LocalMachine,
            path: r"SOFTWARE\Forged\Test".into(),
            value: "Example".into(),
            previous: None,
        };
        assert!(record.describe().starts_with("remove HKLM"));
    }

    #[test]
    fn undo_record_for_present_value_restores() {
        let record = UndoRecord::Registry {
            hive: Hive::CurrentUser,
            path: r"Control Panel\Mouse".into(),
            value: "MouseSpeed".into(),
            previous: Some(RegData::Sz("1".into())),
        };
        let text = record.describe();
        assert!(text.starts_with("restore HKCU"));
        assert!(text.contains("\"1\""));
    }

    #[test]
    fn journal_counts_by_status() {
        let mut j = Journal::new("test".into(), RestorePointResult::Created);
        for (id, status) in [
            ("a", EntryStatus::Applied),
            ("b", EntryStatus::Applied),
            ("c", EntryStatus::Failed),
            ("d", EntryStatus::Skipped),
            ("e", EntryStatus::AlreadyCorrect),
        ] {
            j.push(JournalEntry {
                tweak_id: id.into(),
                tweak_name: id.into(),
                section: Section::System,
                status,
                undo: Vec::new(),
                error: None,
                applied_at: String::new(),
            });
        }
        assert_eq!(j.applied_count(), 2);
        assert_eq!(j.failed_count(), 1);
        assert_eq!(j.skipped_count(), 2);
    }

    #[test]
    fn journal_roundtrips_through_json() {
        let mut j = Journal::new("i5-12400F test rig".into(), RestorePointResult::Created);
        j.push(JournalEntry {
            tweak_id: "net.nagle".into(),
            tweak_name: "Disable Nagle".into(),
            section: Section::Network,
            status: EntryStatus::Applied,
            undo: vec![UndoRecord::Registry {
                hive: Hive::LocalMachine,
                path: "Tcpip".into(),
                value: "TcpAckFrequency".into(),
                previous: None,
            }],
            error: None,
            applied_at: "now".into(),
        });
        j.finish();

        let encoded = serde_json::to_string(&j).expect("serialise");
        let decoded: Journal = serde_json::from_str(&encoded).expect("deserialise");
        assert_eq!(decoded.entries.len(), 1);
        assert_eq!(decoded.entries[0].tweak_id, "net.nagle");
        assert!(decoded.completed_at.is_some());
    }
}
