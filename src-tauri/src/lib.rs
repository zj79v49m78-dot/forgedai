//! Tauri command surface.
//!
//! This layer is deliberately thin: it marshals between the frontend and
//! `forged-core` and does nothing else. Every command is a direct translation of
//! a core function, so the behaviour under test in the core crate is the
//! behaviour the app has.
//!
//! State is held as the last completed scan and plan, because the frontend
//! drives a linear flow (scan → plan → preview → apply → report) and passing
//! large profile structures back and forth across the IPC boundary on every step
//! is wasteful.

use forged_core::{
    ai::{self, OptimisationPlan, ValidationReport},
    bios,
    hardware::HardwareProfile,
    journal::{self, Journal, RevertReport},
    report::{self, Report},
    scan, secure,
    tweaks::{
        catalog,
        engine::{self, ApplyOptions, PlannedChange},
        model::TweakMeta,
    },
    win::elevation,
};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{Manager, State};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Default)]
struct AppState {
    profile: Mutex<Option<HardwareProfile>>,
    plan: Mutex<Option<OptimisationPlan>>,
}

/// Commands return `Result<T, String>` because Tauri needs a serialisable error
/// and the frontend only ever displays the message.
type CmdResult<T> = std::result::Result<T, String>;

fn to_message<T>(result: forged_core::Result<T>) -> CmdResult<T> {
    result.map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Startup / environment
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct Environment {
    version: String,
    elevated: bool,
    has_api_key: bool,
    catalog_size: usize,
    /// Present when a previous run can still be rolled back.
    pending_rollback: Option<String>,
}

#[tauri::command]
fn get_environment() -> CmdResult<Environment> {
    let pending = journal::latest_revertible()
        .ok()
        .flatten()
        .map(|j| report::describe_journal(&j));

    Ok(Environment {
        version: forged_core::VERSION.to_string(),
        elevated: elevation::is_elevated(),
        has_api_key: secure::has_api_key(),
        catalog_size: catalog::count(),
        pending_rollback: pending,
    })
}

// ---------------------------------------------------------------------------
// API key
// ---------------------------------------------------------------------------

#[tauri::command]
fn save_api_key(key: String) -> CmdResult<()> {
    to_message(secure::store_api_key(&key))
}

#[tauri::command]
fn clear_api_key() -> CmdResult<()> {
    to_message(secure::clear_api_key())
}

#[tauri::command]
fn get_masked_api_key() -> CmdResult<Option<String>> {
    Ok(secure::load_api_key().ok().map(|k| secure::masked(&k)))
}

// ---------------------------------------------------------------------------
// Scan
// ---------------------------------------------------------------------------

#[tauri::command]
async fn run_scan(state: State<'_, AppState>) -> CmdResult<HardwareProfile> {
    // The scan shells out to PowerShell several times and takes a few seconds,
    // so it runs on the blocking pool rather than stalling the UI thread.
    let profile = tokio::task::spawn_blocking(scan::scan)
        .await
        .map_err(|e| format!("scan task failed: {e}"))?
        .map_err(|e| e.to_string())?;

    *state.profile.lock().unwrap() = Some(profile.clone());
    Ok(profile)
}

#[tauri::command]
fn get_catalog() -> Vec<TweakMeta> {
    catalog::metadata()
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct PlanResult {
    plan: OptimisationPlan,
    validation: ValidationReport,
    /// True when the plan came from the deterministic fallback rather than the
    /// model. Surfaced in the UI so the user is never misled about it.
    offline: bool,
    /// Why the fallback was used, when it was not simply a missing key. Shown
    /// verbatim so a network or key problem is diagnosable rather than mystifying.
    fallback_reason: Option<String>,
}

#[tauri::command]
async fn build_plan(state: State<'_, AppState>) -> CmdResult<PlanResult> {
    let profile = state
        .profile
        .lock()
        .unwrap()
        .clone()
        .ok_or("run a scan first")?;

    // Falling back is never an error path for the caller: a machine with no key,
    // no internet, or a rejected key still gets a usable plan. Dead-ending the
    // user in front of a completed scan because a network request failed would
    // waste the only part of the run that is hard to redo.
    let fall_back = |reason: String| {
        tracing::warn!("planning offline: {reason}");
        let plan = ai::offline_plan(&profile);
        *state.plan.lock().unwrap() = Some(plan.clone());
        PlanResult {
            plan,
            validation: ValidationReport::default(),
            offline: true,
            fallback_reason: Some(reason),
        }
    };

    let key = match secure::load_api_key() {
        Ok(k) => k,
        // A missing key is an expected state rather than a failure, so it is
        // reported without an alarming reason string.
        Err(_) => return Ok(fall_back("No API key is saved.".into())),
    };

    match ai::plan(&profile, &key).await {
        Ok((plan, validation)) => {
            *state.plan.lock().unwrap() = Some(plan.clone());
            Ok(PlanResult {
                plan,
                validation,
                offline: false,
                fallback_reason: None,
            })
        }
        Err(e) => Ok(fall_back(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Preview and apply
// ---------------------------------------------------------------------------

#[tauri::command]
fn preview_plan(state: State<'_, AppState>) -> CmdResult<Vec<PlannedChange>> {
    let profile = state.profile.lock().unwrap().clone().ok_or("run a scan first")?;
    let plan = state.plan.lock().unwrap().clone().ok_or("build a plan first")?;
    to_message(engine::preview(&profile, &plan.tweak_ids()))
}

#[tauri::command]
async fn apply_plan(
    state: State<'_, AppState>,
    tweak_ids: Option<Vec<String>>,
) -> CmdResult<Report> {
    let profile = state.profile.lock().unwrap().clone().ok_or("run a scan first")?;
    let plan = state.plan.lock().unwrap().clone().ok_or("build a plan first")?;

    // The frontend may pass a user-edited subset; otherwise use the whole plan.
    let ids = tweak_ids.unwrap_or_else(|| plan.tweak_ids());

    let apply_profile = profile.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        engine::apply_plan(&apply_profile, &ids, &ApplyOptions::default())
    })
    .await
    .map_err(|e| format!("apply task failed: {e}"))?
    .map_err(|e| e.to_string())?;

    // Prefer the planner's BIOS list; fall back to the deterministic one so the
    // report always has a firmware section.
    let recommendations = if plan.bios.is_empty() {
        bios::deterministic_recommendations(&profile)
    } else {
        plan.bios.clone()
    };
    let bios_markdown = bios::to_markdown(&recommendations, &profile);

    Ok(report::build(&profile, &plan, &outcome, bios_markdown))
}

// ---------------------------------------------------------------------------
// Rollback
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JournalSummary {
    run_id: String,
    description: String,
    applied: usize,
    reverted: bool,
}

#[tauri::command]
fn list_runs() -> CmdResult<Vec<JournalSummary>> {
    let journals: Vec<Journal> = to_message(journal::list())?;
    Ok(journals
        .into_iter()
        .map(|j| JournalSummary {
            description: report::describe_journal(&j),
            applied: j.applied_count(),
            reverted: j.is_reverted(),
            run_id: j.run_id,
        })
        .collect())
}

#[tauri::command]
async fn rollback_run(run_id: Option<String>) -> CmdResult<RevertReport> {
    tokio::task::spawn_blocking(move || match run_id {
        Some(id) => {
            let mut target = journal::list()?
                .into_iter()
                .find(|j| j.run_id == id)
                .ok_or_else(|| {
                    forged_core::ForgedError::Journal(format!("no run with id {id}"))
                })?;
            journal::revert(&mut target)
        }
        None => journal::revert_latest(),
    })
    .await
    .map_err(|e| format!("rollback task failed: {e}"))?
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

#[tauri::command]
fn export_report(report: Report) -> CmdResult<String> {
    let dir = journal::data_dir().join("reports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path = dir.join(format!("forged-report-{}.md", report.run_id));
    std::fs::write(&path, report.to_markdown()).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "forged=info,forged_core=info".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            app.manage(AppState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_environment,
            save_api_key,
            clear_api_key,
            get_masked_api_key,
            run_scan,
            get_catalog,
            build_plan,
            preview_plan,
            apply_plan,
            list_runs,
            rollback_run,
            export_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Forged");
}
