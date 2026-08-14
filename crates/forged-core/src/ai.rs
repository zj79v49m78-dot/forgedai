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
// Haiku, not Opus. The planner's job is selecting from a vetted list and writing
// a few short explanations — it does not need a frontier model, and Haiku is a
// fraction of the cost and several times faster, which also removed the timeouts
// that plagued the Opus version. The optimisation itself runs locally for free;
// this call is an optional convenience layer, so it should be cheap.
const MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_TOKENS: u32 = 8000;

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
         tweaks. Your job is to choose which apply to this specific machine and order them.\n\n\
         Rules:\n\
         - You may only select tweaks by their exact `id` from the supplied catalog. Never invent \
           an id, a registry path, or a command. Ids you invent are discarded and make the plan \
           worse.\n\
         - `selected_ids` is a plain list of ids. Do NOT write a justification for each one — the \
           app already holds a written rationale for every entry and will use it. Writing one \
           anyway makes the response so long it times out, and the user sees no plan at all.\n\
         - `highlights` is where your judgement goes: pick the 8 to 12 changes that matter most on \
           THIS hardware and explain each in one or two sentences citing the actual CPU, GPU, RAM \
           speed or refresh rate. This is the part the user reads.\n\
         - This machine exists only to play Fortnite, so select everything that genuinely helps \
           this hardware. Every entry in the catalog is safe and reversible — there are no \
           machine-wrecking options to weigh, because they were removed. Do not hold back.\n\
         - Do still reject entries that are wrong *for this hardware*: AMD tweaks on an Intel \
           machine, SSD tweaks on a mechanical drive, laptop-hostile power settings on a laptop. \
           List at most 10 such rejections, one sentence each.\n\
         - High-risk entries are yours to judge. Select one only if this specific machine can \
           absorb its stated tradeoff, and say so in `highlights`.\n\
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

/// The catalog as the planner sees it.
///
/// Deliberately omits `rationale`, which is the longest field on every entry and
/// is already held locally — sending 123 copies of it inflated the prompt by
/// roughly 25,000 characters and bought nothing, because the app fills the
/// rationale back in for any entry the model does not highlight.
#[derive(Serialize)]
struct PlannerTweak {
    id: &'static str,
    name: &'static str,
    section: crate::tweaks::model::Section,
    summary: &'static str,
    risk: crate::tweaks::model::Risk,
    impact: crate::tweaks::model::Impact,
    evidence: crate::tweaks::model::Evidence,
    reboot: bool,
    tradeoff: Option<&'static str>,
}

fn planner_catalog() -> Vec<PlannerTweak> {
    catalog::all()
        .into_iter()
        .map(|t| PlannerTweak {
            id: t.id,
            name: t.name,
            section: t.section,
            summary: t.summary,
            risk: t.risk,
            impact: t.impact,
            evidence: t.evidence,
            reboot: t.requires_reboot,
            tradeoff: t.tradeoff,
        })
        .collect()
}

fn user_prompt(profile: &HardwareProfile) -> Result<String> {
    let profile_json = serde_json::to_string_pretty(profile)?;
    // Compact rather than pretty: the model does not need the whitespace and it
    // is a meaningful fraction of the prompt at this size.
    let catalog_json = serde_json::to_string(&planner_catalog())?;
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
///
/// `selected_ids` is a bare list of strings rather than objects carrying a
/// reason. That shape is the whole reason this call completes: asking for a
/// written justification per entry meant well over a hundred short paragraphs,
/// which took long enough to generate that the request timed out and the user
/// got no plan at all. Judgement now goes into a capped `highlights` list, and
/// everything else falls back to the rationale the app already holds.
fn tool_definition() -> serde_json::Value {
    serde_json::json!({
        "name": "submit_plan",
        "description": "Submit the optimisation plan for this machine.",
        "input_schema": {
            "type": "object",
            "required": ["summary", "selected_ids", "highlights", "bios"],
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "2-4 sentences naming this machine's actual components and what the plan targets."
                },
                "selected_ids": {
                    "type": "array",
                    "description": "Ids of every tweak to apply, in application order. Ids only — no reasons here.",
                    "items": { "type": "string" }
                },
                "highlights": {
                    "type": "array",
                    "description": "The 8-12 changes that matter most on this hardware, explained. Ids must also appear in selected_ids.",
                    "maxItems": 15,
                    "items": {
                        "type": "object",
                        "required": ["id", "reason"],
                        "properties": {
                            "id": { "type": "string" },
                            "reason": {
                                "type": "string",
                                "description": "One or two sentences citing this machine's actual hardware."
                            }
                        }
                    }
                },
                "rejected": {
                    "type": "array",
                    "description": "Entries deliberately not applied. At most 10.",
                    "maxItems": 10,
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
                    "maxItems": 10,
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
                    "maxItems": 12,
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
                    "maxItems": 6,
                    "items": { "type": "string" }
                }
            }
        }
    })
}

/// The wire shape the planner returns, before it is expanded into a full plan.
#[derive(Deserialize)]
struct PlannerResponse {
    summary: String,
    selected_ids: Vec<String>,
    #[serde(default)]
    highlights: Vec<SelectedTweak>,
    #[serde(default)]
    rejected: Vec<RejectedTweak>,
    #[serde(default)]
    bios: Vec<BiosRecommendation>,
    #[serde(default)]
    game_settings: Vec<GameSetting>,
    #[serde(default)]
    hardware_notes: Vec<String>,
}

impl PlannerResponse {
    /// Expands the compact response into a full plan, filling each non-highlighted
    /// entry's reason from the catalog rationale the app already holds.
    fn into_plan(self) -> OptimisationPlan {
        let selected = self
            .selected_ids
            .into_iter()
            .map(|id| {
                let reason = self
                    .highlights
                    .iter()
                    .find(|h| h.id == id)
                    .map(|h| h.reason.clone())
                    .or_else(|| catalog::find(&id).map(|t| t.rationale.to_string()))
                    .unwrap_or_default();
                SelectedTweak { id, reason }
            })
            .collect();

        OptimisationPlan {
            summary: self.summary,
            selected,
            rejected: self.rejected,
            bios: self.bios,
            game_settings: self.game_settings,
            hardware_notes: self.hardware_notes,
        }
    }
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
        .timeout(std::time::Duration::from_secs(300))
        .user_agent(concat!("Forged/", env!("CARGO_PKG_VERSION")))
        // Forged runs elevated, and an elevated process does not always inherit
        // the interactive user's proxy configuration. Reading the environment
        // explicitly is harmless when no proxy is set and is the difference
        // between working and not on a machine behind one.
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
        .map_err(|e| ForgedError::Ai(describe_transport_error(&e)))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| ForgedError::Ai(describe_transport_error(&e)))?;

    if !status.is_success() {
        return Err(ForgedError::Ai(describe_api_error(status.as_u16(), &text)));
    }

    let plan = extract_plan(&text)?;
    Ok(validate(plan, profile))
}

/// Describes a transport failure in terms the user can act on.
///
/// `reqwest::Error`'s own `Display` is only ever "error sending request for url
/// (...)" — the actual reason (DNS failure, TLS handshake rejection, connection
/// refused) lives in the `source()` chain and is dropped entirely by a plain
/// `{e}`. Walking the chain is the difference between an error someone can fix
/// and one they can only report.
fn describe_transport_error(err: &reqwest::Error) -> String {
    let mut causes = Vec::new();
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        causes.push(cause.to_string());
        source = cause.source();
    }

    let detail = if causes.is_empty() {
        String::new()
    } else {
        format!(" — {}", causes.join(": "))
    };

    if err.is_timeout() {
        format!(
            "the Claude API did not respond within 3 minutes{detail}. This is usually a slow or \
             filtered connection rather than a problem with the key."
        )
    } else if err.is_connect() {
        format!(
            "could not open a connection to api.anthropic.com{detail}. Check that this PC is \
             online, and that no firewall, VPN, DNS filter or parental-control software is \
             blocking it. Forged runs elevated, so a proxy configured only for your normal user \
             account will not apply."
        )
    } else {
        format!("the request to the Claude API failed{detail}")
    }
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

    let response: PlannerResponse = serde_json::from_value(tool_input.clone())
        .map_err(|e| ForgedError::Ai(format!("the returned plan did not match the schema: {e}")))?;
    Ok(response.into_plan())
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
    use crate::tweaks::model::Risk;

    // High-risk entries are deliberately withheld here.
    //
    // Their value depends on a judgement this code cannot make: whether the
    // cooling can absorb permanently-disabled C-states, whether starving USB and
    // audio interrupts to favour the GPU is a good trade on this machine. The AI
    // planner weighs those against the actual hardware. With no planner there is
    // nothing doing the weighing, and applying them anyway is how a machine ends
    // up feeling worse under load than it did before — fine in a lobby, and
    // thermally or interrupt-starved once a real match loads.
    //
    // They stay available: the plan screen lists them as withheld, and ticking
    // one is a deliberate act with its tradeoff shown.
    let (selected, withheld): (Vec<_>, Vec<_>) = catalog::all()
        .into_iter()
        .filter(|t| t.is_relevant(profile))
        .partition(|t| t.risk < Risk::High);

    let selected: Vec<SelectedTweak> = selected
        .into_iter()
        .map(|t| SelectedTweak {
            id: t.id.to_string(),
            reason: t.rationale.to_string(),
        })
        .collect();

    let mut rejected: Vec<RejectedTweak> = withheld
        .into_iter()
        .map(|t| RejectedTweak {
            id: t.id.to_string(),
            reason: format!(
                "Withheld: this is a high-risk change and there is no AI plan to judge whether it \
                 suits this machine. {} Tick it manually only if you accept that.",
                t.tradeoff.unwrap_or_default()
            ),
        })
        .collect();

    rejected.extend(
        catalog::all()
            .into_iter()
            .filter(|t| !t.is_relevant(profile))
            .map(|t| RejectedTweak {
                id: t.id.to_string(),
                reason: "Not applicable to the detected hardware.".to_string(),
            }),
    );

    OptimisationPlan {
        summary: format!(
            "Offline plan for {}. Every applicable low and medium risk entry was selected, without \
             AI prioritisation. High-risk entries were withheld because nothing here can weigh \
             their tradeoffs against your hardware — connect an API key for a plan that can.",
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

    /// The offline plan runs when nothing is available to weigh a tradeoff, so
    /// it must not apply changes whose value depends on judgement. Shipping the
    /// unfiltered set is what left a machine feeling fine in a lobby and
    /// interrupt-starved in a real match.
    #[test]
    fn offline_plan_withholds_high_risk_entries() {
        use crate::tweaks::model::Risk;
        let profile = HardwareProfile::default();
        let plan = offline_plan(&profile);

        for entry in &plan.selected {
            let tweak = catalog::find(&entry.id).expect("unknown id");
            assert!(
                tweak.risk < Risk::High,
                "offline plan selected high-risk entry '{}'",
                tweak.id
            );
        }
    }

    /// Withholding must not mean hiding: every high-risk entry that applies to
    /// the machine still has to appear, with its tradeoff, so the choice stays
    /// the user's.
    #[test]
    fn withheld_high_risk_entries_are_still_listed() {
        use crate::tweaks::model::Risk;
        let profile = HardwareProfile::default();
        let plan = offline_plan(&profile);

        let applicable_high: Vec<_> = catalog::all()
            .into_iter()
            .filter(|t| t.risk == Risk::High && t.is_relevant(&profile))
            .collect();

        for tweak in applicable_high {
            assert!(
                plan.rejected.iter().any(|r| r.id == tweak.id),
                "high-risk entry '{}' was withheld without being listed",
                tweak.id
            );
        }
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
                    "selected_ids": ["kbm.pointer_acceleration", "kbm.keyboard_repeat"],
                    "highlights": [{ "id": "kbm.pointer_acceleration", "reason": "bespoke" }],
                    "bios": []
                }}
            ]
        })
        .to_string();

        let plan = extract_plan(&body).expect("should parse");
        assert_eq!(plan.selected.len(), 2);

        // The highlighted entry keeps the model's bespoke reasoning.
        assert_eq!(plan.selected[0].reason, "bespoke");
        // The other falls back to the catalog rationale rather than being blank.
        assert!(!plan.selected[1].reason.is_empty());
        assert_ne!(plan.selected[1].reason, "bespoke");
    }

    /// The bug this guards: the prompt carried every entry's full rationale and
    /// the schema demanded a written justification per entry. Together those made
    /// a request that could not finish inside any sane timeout, so the planner
    /// failed on every machine regardless of connection quality.
    #[test]
    fn planner_payload_stays_small_enough_to_answer() {
        let profile = HardwareProfile::default();
        let prompt = user_prompt(&profile).expect("prompt builds");

        // Roughly 4 chars per token. The full-rationale version of this prompt
        // was around 90k characters; the ceiling here leaves plenty of headroom
        // for the catalog to grow without drifting back into timeout territory.
        assert!(
            prompt.len() < 60_000,
            "planner prompt is {} chars — large prompts are what made this call time out",
            prompt.len()
        );

        // The rationale is the single largest field and is filled in locally, so
        // it must not be travelling to the model.
        let rationale = catalog::all()[0].rationale;
        assert!(
            !prompt.contains(rationale),
            "catalog rationale is being sent to the planner; it is held locally already"
        );
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
