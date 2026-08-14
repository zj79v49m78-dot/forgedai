//! The AI planner.
//!
//! ## What the model is and is not allowed to do
//!
//! The planner receives the machine profile and the catalog's *metadata* — IDs,
//! names, summaries, risk and impact ratings — and returns a selection of IDs
//! with reasoning. It never sees a registry path, and it has no mechanism for
//! returning one. [`validate`] then discards any ID that is not in the catalog
//! before the plan reaches the engine.
//!
//! This is the difference between "the AI is unlikely to break your PC" and "the
//! AI cannot break your PC". A hallucinated tweak ID is caught by a hash lookup;
//! a hallucinated registry write would not be catchable at all. The model's job
//! is judgement — which of 125 vetted changes suit *this* silicon, in what order,
//! and how to explain them — which is what a language model is actually good at.

use crate::error::{ForgedError, Result};
use crate::hardware::HardwareProfile;
use crate::tweaks::catalog;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MODEL: &str = "claude-opus-5";
const MAX_TOKENS: u32 = 16000;

// ---------------------------------------------------------------------------
// Plan shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimisationPlan {
    /// Prose overview shown at the top of the report.
    pub summary: String,
    /// Chosen tweaks, in application order.
    pub selected: Vec<SelectedTweak>,
    /// Catalog entries deliberately not applied, with reasoning. Shown in the
    /// report because "why didn't it do X" is the first question anyone asks.
    pub rejected: Vec<RejectedTweak>,
    /// Firmware changes the user must make by hand.
    pub bios: Vec<BiosRecommendation>,
    /// In-game settings recommendations.
    pub game_settings: Vec<GameSetting>,
    /// Hardware-level observations no software change can address.
    pub hardware_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedTweak {
    pub id: String,
    /// Why this matters on *this* machine specifically.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedTweak {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiosRecommendation {
    pub setting: String,
    pub target_value: String,
    pub reason: String,
    /// Typical menu location, best-effort for the detected board vendor.
    pub where_to_find: String,
    pub priority: BiosPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiosPriority {
    Critical,
    Recommended,
    Optional,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSetting {
    pub setting: String,
    pub value: String,
    pub reason: String,
}

impl OptimisationPlan {
    pub fn tweak_ids(&self) -> Vec<String> {
        self.selected.iter().map(|t| t.id.clone()).collect()
    }

    pub fn reason_for(&self, id: &str) -> Option<&str> {
        self.selected
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.reason.as_str())
    }
}

// ---------------------------------------------------------------------------
// Validation — the safety boundary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    /// IDs the model returned that do not exist in the catalog. Discarded.
    pub unknown_ids: Vec<String>,
    /// IDs dropped because the same entry appeared twice.
    pub duplicates: Vec<String>,
    /// Selected IDs whose applicability predicate says they do not apply here.
    pub not_applicable: Vec<String>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.unknown_ids.is_empty() && self.duplicates.is_empty() && self.not_applicable.is_empty()
    }
}

/// Strips anything the model returned that cannot or should not be executed.
///
/// Returns the cleaned plan alongside a report of what was removed, so the UI
/// can surface it rather than silently swallowing a bad response.
pub fn validate(
    mut plan: OptimisationPlan,
    profile: &HardwareProfile,
) -> (OptimisationPlan, ValidationReport) {
    let mut report = ValidationReport::default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut kept = Vec::with_capacity(plan.selected.len());

    for entry in plan.selected {
        let Some(tweak) = catalog::find(&entry.id) else {
            // The hallucination backstop.
            report.unknown_ids.push(entry.id);
            continue;
        };
        if !seen.insert(entry.id.clone()) {
            report.duplicates.push(entry.id);
            continue;
        }
        if !tweak.is_relevant(profile) {
            report.not_applicable.push(entry.id);
            continue;
        }
        kept.push(entry);
    }

    plan.selected = kept;
    // Rejected entries are advisory text only, but a nonexistent ID there would
    // still render as a broken row in the UI.
    plan.rejected.retain(|r| catalog::find(&r.id).is_some());

    (plan, report)
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

fn system_prompt() -> String {
    format!(
        "You are the planning engine inside Forged, a Windows 11 optimisation tool for a PC used \
         exclusively to play Fortnite competitively.\n\n\
         You are given a hardware profile and a catalog of {} pre-vetted, individually reversible \
         tweaks. Your job is to choose which apply to this specific machine, order them, and \
         explain each one in terms of this machine's actual hardware.\n\n\
         Rules:\n\
         - You may only select tweaks by their exact `id` from the supplied catalog. Never invent \
           an id, a registry path, or a command. Ids you invent are discarded and make the plan \
           worse.\n\
         - The user has chosen the maximum-aggression profile. Select everything that genuinely \
           helps this hardware. Do not hold back low-risk entries.\n\
         - Do still reject entries that are wrong *for this hardware*: AMD tweaks on an Intel \
           machine, SSD tweaks on a mechanical drive, laptop-hostile power settings on a laptop. \
           Explain each rejection in one sentence.\n\
         - Entries marked NoMeasuredBenefit should be selected (they are harmless and expected) \
           but your reason must say plainly that it is included for completeness and is not \
           expected to change performance. Never claim a benefit that is not there.\n\
         - Order matters: power plan changes before per-setting power tweaks, since the latter \
           write into the active scheme.\n\n\
         For the BIOS list, recommend firmware settings the software cannot change. If the profile \
         shows memory running below its rated speed, XMP/EXPO is the single most important item \
         and must be Critical. Tailor `where_to_find` to the detected motherboard vendor.\n\n\
         Be specific and honest. Cite the actual CPU, GPU, RAM speed and refresh rate in your \
         reasoning. Never overstate a gain. The user will read every word of this.",
        catalog::count()
    )
}

fn user_prompt(profile: &HardwareProfile) -> Result<String> {
    let profile_json = serde_json::to_string_pretty(profile)?;
    let catalog_json = serde_json::to_string_pretty(&catalog::metadata())?;
    let findings = profile.blocking_findings();
    let findings_json = serde_json::to_string_pretty(&findings)?;

    Ok(format!(
        "## Machine profile\n\n```json\n{profile_json}\n```\n\n\
         ## Pre-computed hardware findings\n\n\
         These were detected deterministically by the scanner. Incorporate them.\n\n\
         ```json\n{findings_json}\n```\n\n\
         ## Available tweak catalog\n\n```json\n{catalog_json}\n```\n\n\
         Produce the optimisation plan for this machine using the submit_plan tool."
    ))
}

/// The tool schema. Forcing tool use is what makes the response parseable
/// rather than prose we have to scrape.
fn tool_definition() -> serde_json::Value {
    serde_json::json!({
        "name": "submit_plan",
        "description": "Submit the optimisation plan for this machine.",
        "input_schema": {
            "type": "object",
            "required": ["summary", "selected", "rejected", "bios", "game_settings", "hardware_notes"],
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "2-4 sentence overview naming this machine's actual components and what the plan targets."
                },
                "selected": {
                    "type": "array",
                    "description": "Tweaks to apply, in application order.",
                    "items": {
                        "type": "object",
                        "required": ["id", "reason"],
                        "properties": {
                            "id": { "type": "string", "description": "Exact id from the supplied catalog." },
                            "reason": { "type": "string", "description": "One or two sentences on why this matters on this specific hardware." }
                        }
                    }
                },
                "rejected": {
                    "type": "array",
                    "description": "Catalog entries deliberately not applied.",
                    "items": {
                        "type": "object",
                        "required": ["id", "reason"],
                        "properties": {
                            "id": { "type": "string" },
                            "reason": { "type": "string" }
                        }
                    }
                },
                "bios": {
                    "type": "array",
                    "description": "Firmware settings the user must change by hand.",
                    "items": {
                        "type": "object",
                        "required": ["setting", "target_value", "reason", "where_to_find", "priority"],
                        "properties": {
                            "setting": { "type": "string" },
                            "target_value": { "type": "string" },
                            "reason": { "type": "string" },
                            "where_to_find": { "type": "string", "description": "Menu path for the detected board vendor." },
                            "priority": { "type": "string", "enum": ["Critical", "Recommended", "Optional"] }
                        }
                    }
                },
                "game_settings": {
                    "type": "array",
                    "description": "In-game Fortnite settings suited to this hardware.",
                    "items": {
                        "type": "object",
                        "required": ["setting", "value", "reason"],
                        "properties": {
                            "setting": { "type": "string" },
                            "value": { "type": "string" },
                            "reason": { "type": "string" }
                        }
                    }
                },
                "hardware_notes": {
                    "type": "array",
                    "description": "Observations no software change can address.",
                    "items": { "type": "string" }
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// API call
// ---------------------------------------------------------------------------

/// Asks Claude to plan the optimisation for this machine.
pub async fn plan(
    profile: &HardwareProfile,
    api_key: &str,
) -> Result<(OptimisationPlan, ValidationReport)> {
    let body = serde_json::json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "system": system_prompt(),
        "tools": [tool_definition()],
        // Forces the model to answer through the schema instead of prose.
        "tool_choice": { "type": "tool", "name": "submit_plan" },
        "messages": [{
            "role": "user",
            "content": user_prompt(profile)?
        }]
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| ForgedError::Ai(format!("could not create HTTP client: {e}")))?;

    let response = client
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ForgedError::Ai(format!("could not reach the Claude API: {e}")))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| ForgedError::Ai(format!("could not read the API response: {e}")))?;

    if !status.is_success() {
        return Err(ForgedError::Ai(describe_api_error(status.as_u16(), &text)));
    }

    let plan = extract_plan(&text)?;
    Ok(validate(plan, profile))
}

/// Turns an API error into something a user can act on.
fn describe_api_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(300).collect());

    match status {
        401 => format!(
            "the API key was rejected. Check it in Settings — it should start with 'sk-ant-'. ({detail})"
        ),
        429 => format!("rate limited by the API. Wait a moment and run the plan again. ({detail})"),
        400 => format!("the API rejected the request: {detail}"),
        500..=599 => format!("the Claude API is having problems ({status}). Try again shortly. ({detail})"),
        _ => format!("unexpected API response {status}: {detail}"),
    }
}

/// Pulls the tool-use payload out of a Messages API response.
fn extract_plan(body: &str) -> Result<OptimisationPlan> {
    let parsed: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| ForgedError::Ai(format!("API response was not valid JSON: {e}")))?;

    let blocks = parsed
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| ForgedError::Ai("API response had no content blocks".into()))?;

    let tool_input = blocks
        .iter()
        .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .and_then(|b| b.get("input"))
        .ok_or_else(|| {
            // Most commonly a refusal or a max_tokens truncation; surface the
            // stop reason because it tells the user what to do next.
            let stop = parsed
                .get("stop_reason")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            ForgedError::Ai(format!(
                "the model did not return a plan (stop reason: {stop}). If this says \
                 'max_tokens', the catalog response was truncated — report this."
            ))
        })?;

    serde_json::from_value(tool_input.clone())
        .map_err(|e| ForgedError::Ai(format!("the returned plan did not match the schema: {e}")))
}

// ---------------------------------------------------------------------------
// Offline fallback
// ---------------------------------------------------------------------------

/// A deterministic plan used when the API is unreachable or no key is set.
///
/// Selects every catalog entry whose applicability predicate matches the
/// machine. This is strictly worse than the AI plan — it cannot weigh a
/// tradeoff, order by dependency, or explain itself in terms of this hardware —
/// but it means a network outage degrades the product rather than breaking it.
pub fn offline_plan(profile: &HardwareProfile) -> OptimisationPlan {
    let selected: Vec<SelectedTweak> = catalog::all()
        .into_iter()
        .filter(|t| t.is_relevant(profile))
        .map(|t| SelectedTweak {
            id: t.id.to_string(),
            reason: t.rationale.to_string(),
        })
        .collect();

    let rejected: Vec<RejectedTweak> = catalog::all()
        .into_iter()
        .filter(|t| !t.is_relevant(profile))
        .map(|t| RejectedTweak {
            id: t.id.to_string(),
            reason: "Not applicable to the detected hardware.".to_string(),
        })
        .collect();

    OptimisationPlan {
        summary: format!(
            "Offline plan for {}. Every applicable catalog entry was selected without AI \
             prioritisation — connect an API key for a plan tailored to this hardware.",
            profile.summary_line()
        ),
        bios: crate::bios::deterministic_recommendations(profile),
        hardware_notes: profile
            .blocking_findings()
            .into_iter()
            .map(|f| format!("{}: {}", f.title, f.detail))
            .collect(),
        game_settings: Vec::new(),
        selected,
        rejected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_with(ids: &[&str]) -> OptimisationPlan {
        OptimisationPlan {
            summary: "test".into(),
            selected: ids
                .iter()
                .map(|id| SelectedTweak {
                    id: id.to_string(),
                    reason: "test".into(),
                })
                .collect(),
            rejected: Vec::new(),
            bios: Vec::new(),
            game_settings: Vec::new(),
            hardware_notes: Vec::new(),
        }
    }

    /// The core safety property: an ID the model invented never reaches the engine.
    #[test]
    fn hallucinated_ids_are_discarded() {
        let profile = HardwareProfile::default();
        let plan = plan_with(&[
            "kbm.pointer_acceleration",
            "registry.delete_system32",
            "cpu.set_voltage_to_max",
        ]);

        let (cleaned, report) = validate(plan, &profile);
        assert_eq!(report.unknown_ids.len(), 2);
        assert!(report
            .unknown_ids
            .contains(&"registry.delete_system32".to_string()));
        assert!(cleaned
            .selected
            .iter()
            .all(|t| catalog::find(&t.id).is_some()));
    }

    #[test]
    fn duplicate_selections_are_collapsed() {
        let profile = HardwareProfile::default();
        let plan = plan_with(&["kbm.pointer_acceleration", "kbm.pointer_acceleration"]);

        let (cleaned, report) = validate(plan, &profile);
        assert_eq!(cleaned.selected.len(), 1);
        assert_eq!(report.duplicates.len(), 1);
    }

    #[test]
    fn inapplicable_tweaks_are_dropped() {
        // A default profile has no controller, so controller entries must not survive.
        let profile = HardwareProfile::default();
        let plan = plan_with(&["controller.usb_power_management"]);

        let (cleaned, report) = validate(plan, &profile);
        assert!(cleaned.selected.is_empty());
        assert_eq!(report.not_applicable.len(), 1);
    }

    #[test]
    fn rejected_list_is_also_filtered() {
        let profile = HardwareProfile::default();
        let mut plan = plan_with(&[]);
        plan.rejected = vec![
            RejectedTweak {
                id: "gpu.hags_enable".into(),
                reason: "x".into(),
            },
            RejectedTweak {
                id: "not.a.real.id".into(),
                reason: "x".into(),
            },
        ];

        let (cleaned, _) = validate(plan, &profile);
        assert_eq!(cleaned.rejected.len(), 1);
    }

    #[test]
    fn offline_plan_only_contains_applicable_entries() {
        let profile = HardwareProfile::default();
        let plan = offline_plan(&profile);
        for entry in &plan.selected {
            let tweak = catalog::find(&entry.id).expect("offline plan produced an unknown id");
            assert!(tweak.is_relevant(&profile));
        }
    }

    #[test]
    fn api_errors_are_explained_usefully() {
        let msg = describe_api_error(401, r#"{"error":{"message":"invalid x-api-key"}}"#);
        assert!(
            msg.contains("sk-ant-"),
            "401 should tell the user to check the key"
        );

        let msg = describe_api_error(429, "{}");
        assert!(msg.contains("rate limited"));
    }

    #[test]
    fn extract_plan_reads_a_tool_use_block() {
        let body = serde_json::json!({
            "content": [
                { "type": "text", "text": "thinking out loud" },
                { "type": "tool_use", "name": "submit_plan", "input": {
                    "summary": "s",
                    "selected": [{ "id": "kbm.pointer_acceleration", "reason": "r" }],
                    "rejected": [],
                    "bios": [],
                    "game_settings": [],
                    "hardware_notes": []
                }}
            ]
        })
        .to_string();

        let plan = extract_plan(&body).expect("should parse");
        assert_eq!(plan.selected.len(), 1);
    }

    #[test]
    fn extract_plan_surfaces_the_stop_reason_when_no_tool_call() {
        let body = serde_json::json!({
            "content": [{ "type": "text", "text": "..." }],
            "stop_reason": "max_tokens"
        })
        .to_string();

        let err = extract_plan(&body).unwrap_err();
        assert!(err.to_string().contains("max_tokens"));
    }
}
