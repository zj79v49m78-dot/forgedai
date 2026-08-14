//! The post-run report.
//!
//! The honesty rules from the catalog are enforced here rather than left to the
//! UI: entries tagged [`Evidence::NoMeasuredBenefit`] are counted separately and
//! never contribute to the headline number. A tool that says "40 optimisations
//! applied" when twelve of them do nothing is lying by arithmetic, and the
//! person you are showing this to will check.

use crate::ai::OptimisationPlan;
use crate::hardware::{Finding, HardwareProfile};
use crate::journal::{EntryStatus, Journal};
use crate::tweaks::catalog;
use crate::tweaks::engine::ApplyOutcome;
use crate::tweaks::model::{Evidence, Section};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub machine: String,
    pub run_id: String,
    pub generated_at: String,
    pub summary: String,

    /// Changes that are expected to do something.
    pub effective_changes: usize,
    /// Applied, but tagged as having no measured benefit.
    pub neutral_changes: usize,
    pub skipped: usize,
    pub failed: usize,

    pub reboot_required: bool,
    pub restore_point_created: bool,

    pub sections: Vec<SectionReport>,
    pub findings: Vec<Finding>,
    pub failures: Vec<FailureDetail>,
    pub bios_markdown: String,
    pub game_settings: Vec<crate::ai::GameSetting>,
    pub hardware_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionReport {
    pub section: Section,
    pub label: String,
    pub applied: Vec<AppliedChange>,
    pub effective_count: usize,
    pub neutral_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedChange {
    pub id: String,
    pub name: String,
    /// The planner's machine-specific reasoning, falling back to the catalog's
    /// static rationale when running offline.
    pub reason: String,
    pub evidence: Evidence,
    /// False for entries we are explicit about not expecting to help.
    pub expected_to_help: bool,
    pub tradeoff: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureDetail {
    pub id: String,
    pub name: String,
    pub message: String,
}

/// Builds the report from a completed run.
pub fn build(
    profile: &HardwareProfile,
    plan: &OptimisationPlan,
    outcome: &ApplyOutcome,
    bios_markdown: String,
) -> Report {
    let journal = &outcome.journal;
    let mut sections: Vec<SectionReport> = Vec::new();

    for section in Section::all() {
        let applied: Vec<AppliedChange> = journal
            .entries
            .iter()
            .filter(|e| e.section == *section && e.status == EntryStatus::Applied)
            .filter_map(|e| {
                let tweak = catalog::find(&e.tweak_id)?;
                Some(AppliedChange {
                    reason: plan
                        .reason_for(&e.tweak_id)
                        .unwrap_or(tweak.rationale)
                        .to_string(),
                    evidence: tweak.evidence,
                    expected_to_help: tweak.evidence.counts_toward_gains(),
                    tradeoff: tweak.tradeoff.map(|s| s.to_string()),
                    id: e.tweak_id.clone(),
                    name: e.tweak_name.clone(),
                })
            })
            .collect();

        if applied.is_empty() {
            continue;
        }

        sections.push(SectionReport {
            section: *section,
            label: section.label().to_string(),
            effective_count: applied.iter().filter(|c| c.expected_to_help).count(),
            neutral_count: applied.iter().filter(|c| !c.expected_to_help).count(),
            applied,
        });
    }

    let effective_changes: usize = sections.iter().map(|s| s.effective_count).sum();
    let neutral_changes: usize = sections.iter().map(|s| s.neutral_count).sum();

    let failures = journal
        .entries
        .iter()
        .filter(|e| e.status == EntryStatus::Failed)
        .map(|e| FailureDetail {
            id: e.tweak_id.clone(),
            name: e.tweak_name.clone(),
            message: e.error.clone().unwrap_or_else(|| "unknown error".into()),
        })
        .collect();

    Report {
        machine: profile.summary_line(),
        run_id: journal.run_id.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        summary: plan.summary.clone(),
        effective_changes,
        neutral_changes,
        skipped: journal.skipped_count(),
        failed: journal.failed_count(),
        reboot_required: outcome.reboot_required,
        restore_point_created: journal.restore_point.created(),
        sections,
        findings: profile.blocking_findings(),
        failures,
        bios_markdown,
        game_settings: plan.game_settings.clone(),
        hardware_notes: plan.hardware_notes.clone(),
    }
}

impl Report {
    /// The headline claim. Deliberately counts only changes we stand behind.
    pub fn headline(&self) -> String {
        let mut text = format!(
            "{} performance changes applied across {} areas",
            self.effective_changes,
            self.sections.len()
        );
        if self.neutral_changes > 0 {
            text.push_str(&format!(
                ", plus {} applied for completeness that are not expected to change performance",
                self.neutral_changes
            ));
        }
        text
    }

    /// Full Markdown export, for the "save report" button.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();

        out.push_str("# Forged optimisation report\n\n");
        out.push_str(&format!("**Machine:** {}\n\n", self.machine));
        out.push_str(&format!("**Run:** {}\n\n", self.run_id));
        out.push_str(&format!("{}\n\n", self.summary));
        out.push_str(&format!("{}\n\n", self.headline()));

        if self.reboot_required {
            out.push_str(
                "> **A restart is required** for some of these changes to take effect.\n\n",
            );
        }
        if !self.restore_point_created {
            out.push_str(
                "> **Note:** a System Restore point could not be created. Forged's own rollback \
                 still works and is more precise; this only removes the firmware-level fallback.\n\n",
            );
        }

        if !self.findings.is_empty() {
            out.push_str("## Findings that matter more than any software tweak\n\n");
            for finding in &self.findings {
                out.push_str(&format!(
                    "- **{}** ({:?}) — {}\n",
                    finding.title, finding.severity, finding.detail
                ));
            }
            out.push('\n');
        }

        for section in &self.sections {
            out.push_str(&format!("## {}\n\n", section.label));
            for change in &section.applied {
                let marker = if change.expected_to_help { "✓" } else { "·" };
                out.push_str(&format!("{marker} **{}**\n\n", change.name));
                out.push_str(&format!("  {}\n\n", change.reason));
                if let Some(tradeoff) = &change.tradeoff {
                    out.push_str(&format!("  *Tradeoff: {tradeoff}*\n\n"));
                }
            }
        }

        if !self.failures.is_empty() {
            out.push_str("## Changes that failed\n\n");
            for failure in &self.failures {
                out.push_str(&format!("- **{}** — {}\n", failure.name, failure.message));
            }
            out.push('\n');
        }

        if !self.game_settings.is_empty() {
            out.push_str("## Recommended in-game settings\n\n");
            for setting in &self.game_settings {
                out.push_str(&format!(
                    "- **{}**: {} — {}\n",
                    setting.setting, setting.value, setting.reason
                ));
            }
            out.push('\n');
        }

        out.push_str(&self.bios_markdown);
        out.push_str(
            "\n---\n\n*Every change above is reversible from Forged's Rollback screen.*\n",
        );

        out
    }
}

/// Report for a run that produced nothing, so the UI never renders an empty shell.
pub fn empty(profile: &HardwareProfile, reason: &str) -> Report {
    Report {
        machine: profile.summary_line(),
        run_id: String::new(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        summary: reason.to_string(),
        effective_changes: 0,
        neutral_changes: 0,
        skipped: 0,
        failed: 0,
        reboot_required: false,
        restore_point_created: false,
        sections: Vec::new(),
        findings: profile.blocking_findings(),
        failures: Vec::new(),
        bios_markdown: String::new(),
        game_settings: Vec::new(),
        hardware_notes: Vec::new(),
    }
}

/// Convenience for the rollback screen: a plain-language description of a
/// previous run.
pub fn describe_journal(journal: &Journal) -> String {
    format!(
        "{} — {} changes applied, {} skipped, {} failed{}",
        journal.started_at,
        journal.applied_count(),
        journal.skipped_count(),
        journal.failed_count(),
        if journal.is_reverted() {
            " (already rolled back)"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::SelectedTweak;
    use crate::journal::JournalEntry;
    use crate::win::elevation::RestorePointResult;

    fn outcome_with(entries: Vec<(&str, &str, Section, EntryStatus)>) -> ApplyOutcome {
        let mut journal = Journal::new("test machine".into(), RestorePointResult::Created);
        for (id, name, section, status) in entries {
            journal.push(JournalEntry {
                tweak_id: id.into(),
                tweak_name: name.into(),
                section,
                status,
                undo: Vec::new(),
                error: if status == EntryStatus::Failed {
                    Some("access denied".into())
                } else {
                    None
                },
                applied_at: String::new(),
            });
        }
        ApplyOutcome {
            journal,
            reboot_required: false,
            applied: Vec::new(),
            skipped: Vec::new(),
            failures: Vec::new(),
        }
    }

    fn plan_for(ids: &[&str]) -> OptimisationPlan {
        OptimisationPlan {
            summary: "Test plan.".into(),
            selected: ids
                .iter()
                .map(|id| SelectedTweak {
                    id: id.to_string(),
                    reason: format!("because of {id}"),
                })
                .collect(),
            rejected: Vec::new(),
            bios: Vec::new(),
            game_settings: Vec::new(),
            hardware_notes: Vec::new(),
        }
    }

    /// The central honesty property: placebo entries never inflate the headline.
    #[test]
    fn placebo_entries_do_not_count_toward_the_headline() {
        let profile = HardwareProfile::default();
        let outcome = outcome_with(vec![
            (
                "kbm.pointer_acceleration",
                "Disable mouse acceleration",
                Section::KeyboardMouse,
                EntryStatus::Applied,
            ),
            (
                "kbm.mouse_data_queue_size",
                "Mouse buffer size",
                Section::KeyboardMouse,
                EntryStatus::Applied,
            ),
        ]);

        let report = build(&profile, &plan_for(&[]), &outcome, String::new());

        assert_eq!(report.effective_changes, 1, "only the real change counts");
        assert_eq!(report.neutral_changes, 1);
        assert!(report.headline().contains("1 performance changes"));
        assert!(report
            .headline()
            .contains("not expected to change performance"));
    }

    #[test]
    fn planner_reasoning_overrides_the_static_rationale() {
        let profile = HardwareProfile::default();
        let outcome = outcome_with(vec![(
            "kbm.pointer_acceleration",
            "Disable mouse acceleration",
            Section::KeyboardMouse,
            EntryStatus::Applied,
        )]);

        let report = build(
            &profile,
            &plan_for(&["kbm.pointer_acceleration"]),
            &outcome,
            String::new(),
        );

        let change = &report.sections[0].applied[0];
        assert_eq!(change.reason, "because of kbm.pointer_acceleration");
    }

    #[test]
    fn failures_are_reported_not_hidden() {
        let profile = HardwareProfile::default();
        let outcome = outcome_with(vec![(
            "cpu.disable_vbs",
            "Disable VBS",
            Section::Cpu,
            EntryStatus::Failed,
        )]);

        let report = build(&profile, &plan_for(&[]), &outcome, String::new());
        assert_eq!(report.failed, 1);
        assert_eq!(report.failures.len(), 1);
        assert!(report.to_markdown().contains("Changes that failed"));
    }

    #[test]
    fn sections_with_nothing_applied_are_omitted() {
        let profile = HardwareProfile::default();
        let outcome = outcome_with(vec![(
            "kbm.pointer_acceleration",
            "Disable mouse acceleration",
            Section::KeyboardMouse,
            EntryStatus::Applied,
        )]);

        let report = build(&profile, &plan_for(&[]), &outcome, String::new());
        assert_eq!(report.sections.len(), 1);
        assert_eq!(report.sections[0].section, Section::KeyboardMouse);
    }

    #[test]
    fn missing_restore_point_is_disclosed_in_the_export() {
        let profile = HardwareProfile::default();
        let mut outcome = outcome_with(vec![]);
        outcome.journal.restore_point = RestorePointResult::Unavailable("disabled".into());

        let report = build(&profile, &plan_for(&[]), &outcome, String::new());
        assert!(!report.restore_point_created);
        assert!(report
            .to_markdown()
            .contains("System Restore point could not be created"));
    }
}
