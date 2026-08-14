//! The vetted tweak catalog.
//!
//! Every change Forged is capable of making lives here, as a `static` array of
//! declarations. Nothing is generated at runtime and nothing is read from a
//! config file, which means the complete set of things this application can do
//! to a machine is enumerable by reading these files — by a reviewer, and by the
//! test suite.
//!
//! ## On honesty
//!
//! A large fraction of the "gaming optimisation" advice in circulation does
//! nothing. Rather than quietly omit those entries — users look for them, and
//! their absence reads as an incomplete tool — they are included and tagged
//! [`Evidence::NoMeasuredBenefit`]. The report separates them from the changes
//! that are actually doing work. A tool that claims 40 improvements when 12 are
//! real is worse than one that says so.

use crate::tweaks::model::{Section, Tweak, TweakMeta};

pub mod controller;
pub mod cpu;
pub mod fortnite;
pub mod gpu;
pub mod kbm;
pub mod latency;
pub mod memory;
pub mod network;
pub mod storage;
pub mod system;

/// All catalog sections, in the order they appear in the UI.
fn sections() -> [&'static [Tweak]; 10] {
    [
        controller::TWEAKS,
        kbm::TWEAKS,
        network::TWEAKS,
        gpu::TWEAKS,
        cpu::TWEAKS,
        memory::TWEAKS,
        storage::TWEAKS,
        system::TWEAKS,
        latency::TWEAKS,
        fortnite::TWEAKS,
    ]
}

/// Every tweak in the catalog.
pub fn all() -> Vec<&'static Tweak> {
    sections().into_iter().flatten().collect()
}

/// Resolve an ID. Returns `None` for anything not in the catalog — this is what
/// makes an AI-produced plan safe to execute.
pub fn find(id: &str) -> Option<&'static Tweak> {
    all().into_iter().find(|t| t.id == id)
}

pub fn by_section(section: Section) -> Vec<&'static Tweak> {
    all().into_iter().filter(|t| t.section == section).collect()
}

/// The serialisable view sent to the AI planner and rendered in the UI.
pub fn metadata() -> Vec<TweakMeta> {
    all().into_iter().map(|t| t.metadata()).collect()
}

/// Total number of vetted entries, shown on the landing screen.
pub fn count() -> usize {
    sections().into_iter().map(|s| s.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tweaks::model::{Evidence, Risk};
    use std::collections::HashSet;

    /// The engine resolves plans by ID; a duplicate would make resolution
    /// ambiguous and silently drop one of the two entries.
    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for tweak in all() {
            assert!(
                seen.insert(tweak.id),
                "duplicate tweak id in catalog: {}",
                tweak.id
            );
        }
    }

    /// IDs are `section.name`, which the UI relies on for grouping and the
    /// planner prompt uses to keep selections legible.
    #[test]
    fn ids_follow_naming_convention() {
        for tweak in all() {
            assert!(
                tweak.id.contains('.'),
                "tweak id '{}' should be dot-namespaced",
                tweak.id
            );
            assert!(
                tweak
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_' || c.is_ascii_digit()),
                "tweak id '{}' must be lowercase ascii",
                tweak.id
            );
        }
    }

    /// A high-risk change with no stated tradeoff cannot be disclosed properly
    /// in the UI, which would defeat the point of the risk rating.
    #[test]
    fn high_risk_tweaks_declare_a_tradeoff() {
        for tweak in all() {
            if tweak.risk == Risk::High {
                assert!(
                    tweak.tradeoff.is_some(),
                    "high-risk tweak '{}' must declare a tradeoff",
                    tweak.id
                );
            }
        }
    }

    /// Every entry has to explain itself; these strings go straight into the
    /// report the user reads.
    #[test]
    fn all_entries_are_documented() {
        for tweak in all() {
            assert!(!tweak.name.is_empty(), "{} has no name", tweak.id);
            assert!(
                tweak.summary.len() > 20,
                "{} needs a real summary",
                tweak.id
            );
            assert!(
                tweak.rationale.len() > 30,
                "{} needs a real rationale",
                tweak.id
            );
        }
    }

    /// Guards the honesty policy: the catalog must keep carrying entries we know
    /// are folklore, tagged as such, rather than quietly dropping or promoting
    /// them.
    #[test]
    fn placebo_entries_are_labelled_and_present() {
        let placebo: Vec<_> = all()
            .into_iter()
            .filter(|t| t.evidence == Evidence::NoMeasuredBenefit)
            .collect();
        assert!(
            !placebo.is_empty(),
            "catalog should retain known-ineffective entries, labelled honestly"
        );
        for tweak in placebo {
            assert_eq!(
                tweak.risk,
                Risk::Low,
                "'{}' has no measured benefit, so it must not carry risk",
                tweak.id
            );
        }
    }

    /// Command actions are the only ones the engine cannot introspect, so their
    /// declared inverse is what makes them reversible.
    #[test]
    fn command_actions_declare_a_revert() {
        use crate::tweaks::model::Action;
        let profile = crate::hardware::HardwareProfile::default();
        for tweak in all() {
            for action in tweak.actions_for(&profile) {
                if let Action::RunCommand { revert, .. } = action {
                    assert!(
                        !revert.program.is_empty(),
                        "{} has a command action with an empty revert",
                        tweak.id
                    );
                }
            }
        }
    }

    /// Writing a key's *default* value (empty value name) creates the key as a
    /// side effect, and undo can only delete the value — leaving an empty key
    /// behind. For tweaks that work by key presence, that means rollback
    /// silently fails to restore the original behaviour. Named values only.
    #[test]
    fn no_action_writes_a_default_value() {
        use crate::tweaks::model::Action;
        let profile = crate::hardware::HardwareProfile::default();

        for tweak in all() {
            for action in tweak.actions_for(&profile) {
                let name = match &action {
                    Action::SetRegistry { value, .. } => Some(value.clone()),
                    Action::DeleteRegistryValue { value, .. } => Some(value.clone()),
                    _ => None,
                };
                if let Some(name) = name {
                    assert!(
                        !name.is_empty(),
                        "{} writes a key default value, which rollback cannot undo",
                        tweak.id
                    );
                }
            }
        }
    }

    #[test]
    fn catalog_is_substantial() {
        assert!(
            count() >= 100,
            "catalog has only {} entries; expected a comprehensive set",
            count()
        );
    }

    /// Building actions against a blank profile must not panic — the planner and
    /// preview screen both do this before a scan completes.
    #[test]
    fn actions_build_against_empty_profile() {
        let profile = crate::hardware::HardwareProfile::default();
        for tweak in all() {
            let _ = tweak.is_relevant(&profile);
            let _ = tweak.actions_for(&profile);
        }
    }
}
