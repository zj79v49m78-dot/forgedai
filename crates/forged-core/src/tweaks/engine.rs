//! Plan execution.
//!
//! The engine is the only component that mutates the machine. It takes a list of
//! tweak IDs — from the AI planner, or from the user's manual selection — and
//! runs them under three invariants:
//!
//! * every ID is resolved against the catalog first, so an unknown or invented
//!   ID aborts the run before anything is touched;
//! * every action's prior state is captured and journalled before the action is
//!   reported as successful;
//! * a failure in one tweak never aborts the run, because stopping halfway with
//!   an incomplete journal is the one genuinely dangerous outcome.

use crate::error::{ForgedError, Result, TweakFailure};
use crate::hardware::HardwareProfile;
use crate::journal::{EntryStatus, Journal, JournalEntry, UndoRecord};
use crate::tweaks::catalog;
use crate::tweaks::model::{Action, Tweak};
use crate::win::{elevation, process, registry, service};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Options and results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyOptions {
    /// Resolve and report the full action list without touching anything.
    /// Powers the "Preview changes" screen.
    pub dry_run: bool,
    /// Attempt a System Restore checkpoint before the first write.
    pub create_restore_point: bool,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            create_restore_point: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyOutcome {
    pub journal: Journal,
    pub reboot_required: bool,
    pub applied: Vec<String>,
    pub skipped: Vec<String>,
    pub failures: Vec<TweakFailure>,
}

impl ApplyOutcome {
    pub fn success_rate(&self) -> f32 {
        let total = self.applied.len() + self.failures.len();
        if total == 0 {
            return 1.0;
        }
        self.applied.len() as f32 / total as f32
    }
}

/// One resolved action, for the dry-run preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedChange {
    pub tweak_id: String,
    pub tweak_name: String,
    pub description: String,
    /// False when the machine already holds the desired value.
    pub would_change: bool,
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Maps requested IDs onto catalog entries.
///
/// This is the security boundary for AI-produced plans: an ID that is not in the
/// catalog cannot be executed, it aborts the run. The planner is therefore
/// structurally unable to author a registry write, only to choose among vetted
/// ones.
pub fn resolve<'a>(tweak_ids: &[String]) -> Result<Vec<&'a Tweak>> {
    let mut resolved = Vec::with_capacity(tweak_ids.len());
    for id in tweak_ids {
        let tweak = catalog::find(id).ok_or_else(|| ForgedError::UnknownTweak(id.clone()))?;
        resolved.push(tweak);
    }
    Ok(resolved)
}

/// Builds the full change list without executing, for preview.
pub fn preview(profile: &HardwareProfile, tweak_ids: &[String]) -> Result<Vec<PlannedChange>> {
    let tweaks = resolve(tweak_ids)?;
    let mut out = Vec::new();

    for tweak in tweaks {
        if !tweak.is_relevant(profile) {
            continue;
        }
        for action in tweak.actions_for(profile) {
            let would_change = !action_already_satisfied(&action).unwrap_or(false);
            out.push(PlannedChange {
                tweak_id: tweak.id.to_string(),
                tweak_name: tweak.name.to_string(),
                description: action.describe(),
                would_change,
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// Applies a plan, returning the completed journal.
pub fn apply_plan(
    profile: &HardwareProfile,
    tweak_ids: &[String],
    options: &ApplyOptions,
) -> Result<ApplyOutcome> {
    // Resolve everything up front so an unknown ID fails before any mutation.
    let tweaks = resolve(tweak_ids)?;

    if options.dry_run {
        let journal = Journal::new(profile.summary_line(), elevation::RestorePointResult::Created);
        return Ok(ApplyOutcome {
            journal,
            reboot_required: tweaks.iter().any(|t| t.requires_reboot),
            applied: Vec::new(),
            skipped: tweak_ids.to_vec(),
            failures: Vec::new(),
        });
    }

    elevation::require_elevation()?;

    let restore_point = if options.create_restore_point {
        elevation::try_create_restore_point(&format!("Forged {} — before optimisation", crate::VERSION))
    } else {
        elevation::RestorePointResult::Unavailable("skipped by request".into())
    };

    let mut journal = Journal::new(profile.summary_line(), restore_point);
    journal.save()?; // establish the file before the first write

    let mut applied = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    let mut reboot_required = false;

    for tweak in tweaks {
        if !tweak.is_relevant(profile) {
            journal.push(skip_entry(tweak, "not applicable to this hardware"));
            skipped.push(tweak.id.to_string());
            journal.save()?;
            continue;
        }

        let entry = apply_tweak(tweak, profile);

        match entry.status {
            EntryStatus::Applied => {
                applied.push(tweak.id.to_string());
                if tweak.requires_reboot {
                    reboot_required = true;
                }
            }
            EntryStatus::Failed => {
                failures.push(TweakFailure {
                    tweak_id: tweak.id.to_string(),
                    stage: "apply".into(),
                    message: entry.error.clone().unwrap_or_default(),
                });
            }
            EntryStatus::Skipped | EntryStatus::AlreadyCorrect => {
                skipped.push(tweak.id.to_string());
            }
        }

        journal.push(entry);
        // Persist after every tweak: a power cut mid-run must still leave a
        // complete record of what was already changed.
        journal.save()?;
    }

    journal.finish();
    journal.save()?;

    Ok(ApplyOutcome {
        journal,
        reboot_required,
        applied,
        skipped,
        failures,
    })
}

/// Applies one tweak, collecting undo records as it goes.
///
/// If any action in a tweak fails, the actions already performed within that
/// same tweak are rolled back immediately, so a tweak is all-or-nothing rather
/// than leaving the machine in a state that matches neither before nor after.
fn apply_tweak(tweak: &Tweak, profile: &HardwareProfile) -> JournalEntry {
    let actions = tweak.actions_for(profile);
    let now = chrono::Utc::now().to_rfc3339();

    if actions.is_empty() {
        return JournalEntry {
            tweak_id: tweak.id.to_string(),
            tweak_name: tweak.name.to_string(),
            section: tweak.section,
            status: EntryStatus::Skipped,
            undo: Vec::new(),
            error: Some("no actions resolved for this machine".into()),
            applied_at: now,
        };
    }

    let mut undo: Vec<UndoRecord> = Vec::new();
    let mut changed_anything = false;

    for action in &actions {
        match perform(action) {
            Ok(ActionResult::Changed(record)) => {
                changed_anything = true;
                if let Some(r) = record {
                    undo.push(r);
                }
            }
            Ok(ActionResult::AlreadyCorrect) => {}
            Ok(ActionResult::TargetMissing) => {}
            Err(e) => {
                // Unwind this tweak's own changes before reporting failure.
                for record in undo.iter().rev() {
                    let _ = record.apply();
                }
                return JournalEntry {
                    tweak_id: tweak.id.to_string(),
                    tweak_name: tweak.name.to_string(),
                    section: tweak.section,
                    status: EntryStatus::Failed,
                    undo: Vec::new(),
                    error: Some(e.to_string()),
                    applied_at: now,
                };
            }
        }
    }

    JournalEntry {
        tweak_id: tweak.id.to_string(),
        tweak_name: tweak.name.to_string(),
        section: tweak.section,
        status: if changed_anything {
            EntryStatus::Applied
        } else {
            EntryStatus::AlreadyCorrect
        },
        undo,
        error: None,
        applied_at: now,
    }
}

fn skip_entry(tweak: &Tweak, reason: &str) -> JournalEntry {
    JournalEntry {
        tweak_id: tweak.id.to_string(),
        tweak_name: tweak.name.to_string(),
        section: tweak.section,
        status: EntryStatus::Skipped,
        undo: Vec::new(),
        error: Some(reason.to_string()),
        applied_at: chrono::Utc::now().to_rfc3339(),
    }
}

enum ActionResult {
    /// Changed; carries the undo record where one is needed.
    Changed(Option<UndoRecord>),
    AlreadyCorrect,
    /// The service or device this action targets is not present on this machine.
    TargetMissing,
}

/// Executes a single action and returns its inverse.
fn perform(action: &Action) -> Result<ActionResult> {
    match action {
        Action::SetRegistry {
            hive,
            path,
            value,
            data,
        } => {
            if registry::get_value(*hive, path, value)?.as_ref() == Some(data) {
                return Ok(ActionResult::AlreadyCorrect);
            }
            let previous = registry::set_value(*hive, path, value, data)?;
            Ok(ActionResult::Changed(Some(UndoRecord::Registry {
                hive: *hive,
                path: path.clone(),
                value: value.clone(),
                previous,
            })))
        }

        Action::DeleteRegistryValue { hive, path, value } => {
            let existing = registry::get_value(*hive, path, value)?;
            if existing.is_none() {
                return Ok(ActionResult::AlreadyCorrect);
            }
            let previous = registry::delete_value(*hive, path, value)?;
            Ok(ActionResult::Changed(Some(UndoRecord::Registry {
                hive: *hive,
                path: path.clone(),
                value: value.clone(),
                previous,
            })))
        }

        Action::SetServiceStart {
            service: name,
            start,
        } => {
            if !service::exists(name)? {
                return Ok(ActionResult::TargetMissing);
            }
            if service::get_start(name)? == Some(*start) {
                return Ok(ActionResult::AlreadyCorrect);
            }
            match service::set_start(name, *start)? {
                Some(previous) => {
                    // Stopping is best-effort; the start type is what matters at boot.
                    let _ = service::try_stop(name);
                    Ok(ActionResult::Changed(Some(UndoRecord::Service {
                        name: name.clone(),
                        previous,
                    })))
                }
                None => Ok(ActionResult::TargetMissing),
            }
        }

        Action::RunCommand {
            program,
            args,
            revert,
            tolerate_exit_codes,
        } => {
            process::run_tolerant(program, args, tolerate_exit_codes)?;
            Ok(ActionResult::Changed(Some(UndoRecord::Command {
                program: revert.program.clone(),
                args: revert.args.clone(),
            })))
        }
    }
}

/// Whether the machine already satisfies an action, for the preview screen.
/// Command actions are always reported as "would change" because they cannot be
/// introspected without running them.
fn action_already_satisfied(action: &Action) -> Result<bool> {
    Ok(match action {
        Action::SetRegistry {
            hive,
            path,
            value,
            data,
        } => registry::get_value(*hive, path, value)?.as_ref() == Some(data),
        Action::DeleteRegistryValue { hive, path, value } => {
            registry::get_value(*hive, path, value)?.is_none()
        }
        Action::SetServiceStart {
            service: name,
            start,
        } => service::get_start(name)? == Some(*start),
        Action::RunCommand { .. } => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tweak_id_is_rejected() {
        let err = resolve(&["definitely.not.a.real.tweak".to_string()]).unwrap_err();
        assert!(matches!(err, ForgedError::UnknownTweak(_)));
    }

    #[test]
    fn every_catalog_id_resolves() {
        let ids: Vec<String> = catalog::all().iter().map(|t| t.id.to_string()).collect();
        assert!(resolve(&ids).is_ok(), "catalog contains an unresolvable id");
    }

    #[test]
    fn plan_with_one_bad_id_aborts_before_touching_anything() {
        let mut ids: Vec<String> = catalog::all().iter().take(3).map(|t| t.id.to_string()).collect();
        ids.push("ai.hallucinated.entry".into());
        assert!(resolve(&ids).is_err());
    }
}
