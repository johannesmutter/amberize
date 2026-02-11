use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use email_archiver_adapters::{
    sync_account_once_with_progress, KeychainSecretStore, SyncProgressFn,
};
use email_archiver_storage::{
    InsertEventInput, Storage, EVENT_KIND_INTEGRITY_CHECK, EVENT_KIND_TAMPERING_DETECTED,
};

use crate::app_state::{AppState, UiSyncStatus};
use crate::sync_status_text::format_last_sync_status_text;

const BACKGROUND_SYNC_POLL_NOT_CONFIGURED: Duration = Duration::from_secs(60);
const BACKGROUND_SYNC_INITIAL_DELAY: Duration = Duration::from_secs(60);

/// Run a full event chain verification every N sync cycles.
const FULL_VERIFICATION_EVERY_N_CYCLES: u64 = 10;

const EVENT_SYNC_STATUS_UPDATED: &str = "sync_status_updated";
const EVENT_SYNC_PROGRESS: &str = "sync_progress";

const EVENT_KIND_APP_STARTED: &str = "app_started";
const EVENT_KIND_COVERAGE_GAP: &str = "coverage_gap";

use email_archiver_storage::EVENT_KIND_SYNC_FINISHED;

pub fn start_background_sync(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(BACKGROUND_SYNC_INITIAL_DELAY).await;

        let mut sync_cycle_count: u64 = 0;

        loop {
            let start = tokio::time::Instant::now();
            let result = run_sync_all_accounts_once(&app_handle).await;
            let elapsed = start.elapsed();

            // After each successful sync, run periodic integrity checks.
            if result.is_ok() {
                sync_cycle_count += 1;
                let run_full = sync_cycle_count.is_multiple_of(FULL_VERIFICATION_EVERY_N_CYCLES);
                run_periodic_integrity_check(&app_handle, run_full);
            }

            let target_interval = match &result {
                Ok(status)
                    if status.last_sync_at.is_none()
                        && status.last_sync_status == "not configured" =>
                {
                    BACKGROUND_SYNC_POLL_NOT_CONFIGURED
                }
                _ => {
                    // Read the user-configured interval from AppState.
                    let state = app_handle.state::<AppState>();
                    Duration::from_secs(state.sync_interval_secs())
                }
            };

            // Subtract elapsed sync time to compensate for timer drift.
            let sleep_for = target_interval.saturating_sub(elapsed);
            tokio::time::sleep(sleep_for).await;
        }
    });
}

// ---------------------------------------------------------------------------
// Startup & coverage gap detection
// ---------------------------------------------------------------------------

/// Called once during app startup (from `set_active_db_path`).
///
/// 1. Records an `app_started` event.
/// 2. Queries the last heartbeat.
/// 3. Compares it against system boot time to detect periods where the
///    computer was on but the app was not running, and logs any such gaps as
///    `coverage_gap` events in the tamper-evident chain.
pub fn record_startup_and_detect_gaps(app_handle: &AppHandle) {
    let Some(db_path) = get_active_db_path(app_handle) else {
        return;
    };
    let Ok(storage) = Storage::open_or_create(&db_path) else {
        return;
    };

    let now = now_rfc3339();

    // 1. Record app_started.
    let boot_time_rfc = get_system_boot_time_rfc3339().unwrap_or_default();
    let _ = storage.append_event(&InsertEventInput {
        occurred_at: now.clone(),
        kind: EVENT_KIND_APP_STARTED.to_string(),
        account_id: None,
        mailbox_id: None,
        message_blob_id: None,
        detail: format!(
            r#"{{"v":1,"system_boot_time":"{}"}}"#,
            escape_json_value(&boot_time_rfc)
        ),
    });

    // 2. Check for a coverage gap.
    //    Use the last `sync_finished` event as the best indicator of when the
    //    app was last active.  Fall back to `app_started` for first-run cases.
    let last_heartbeat = get_last_event_time(&storage, EVENT_KIND_SYNC_FINISHED)
        .or_else(|| get_last_event_time(&storage, EVENT_KIND_APP_STARTED));

    let Some(last_hb) = last_heartbeat else {
        // First run ever — no gap to report.
        return;
    };

    // Parse timestamps to compare.
    let Ok(last_hb_dt) =
        time::OffsetDateTime::parse(&last_hb, &time::format_description::well_known::Rfc3339)
    else {
        return;
    };
    let Ok(now_dt) =
        time::OffsetDateTime::parse(&now, &time::format_description::well_known::Rfc3339)
    else {
        return;
    };

    // The gap is the period between the last heartbeat and now.
    // If the system was booted after the last heartbeat, the computer was
    // off for part of that period — only the time from boot to now is
    // "uncovered."
    let gap_start = if let Some(boot_dt) = parse_rfc3339(&boot_time_rfc) {
        // The computer was off between last_hb and boot.  The uncovered
        // gap is from boot to now (app wasn't running while system was on).
        if boot_dt > last_hb_dt {
            boot_dt
        } else {
            // System has been on since before the last heartbeat — the
            // entire period from last_hb to now is uncovered.
            last_hb_dt
        }
    } else {
        last_hb_dt
    };

    let gap_seconds = (now_dt - gap_start).whole_seconds();

    // Only report gaps longer than a reasonable threshold (2× the default
    // sync interval, i.e. 30 min) to avoid noisy events from normal restarts.
    const GAP_THRESHOLD_SECS: i64 = 30 * 60;
    if gap_seconds <= GAP_THRESHOLD_SECS {
        return;
    }

    let gap_start_rfc = gap_start
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    let _ = storage.append_event(&InsertEventInput {
        occurred_at: now,
        kind: EVENT_KIND_COVERAGE_GAP.to_string(),
        account_id: None,
        mailbox_id: None,
        message_blob_id: None,
        detail: format!(
            r#"{{"v":1,"gap_start":"{}","gap_end_approx":"{}","gap_seconds":{},"last_heartbeat":"{}","system_boot_time":"{}"}}"#,
            escape_json_value(&gap_start_rfc),
            escape_json_value(&now_rfc3339()),
            gap_seconds,
            escape_json_value(&last_hb),
            escape_json_value(&boot_time_rfc),
        ),
    });
}

/// Run a full integrity verification on startup.
///
/// Verifies the event hash chain and compares the current root hash against
/// the last `sync_finished` checkpoint.  If tampering is detected, a
/// `tampering_detected` event is recorded and the result is stored in the
/// app state for the UI to query.
pub fn verify_integrity_at_startup(app_handle: &AppHandle) {
    let Some(db_path) = get_active_db_path(app_handle) else {
        return;
    };
    let Ok(storage) = Storage::open_or_create(&db_path) else {
        return;
    };

    let status = match storage.verify_integrity() {
        Ok(s) => s,
        Err(_) => return,
    };

    // Persist the result so the UI can query it.
    let state = app_handle.state::<AppState>();
    if let Ok(mut guard) = state.integrity_status.lock() {
        *guard = Some(status.clone());
    }

    if status.ok {
        // Record a clean integrity_check event.
        let _ = storage.append_event(&InsertEventInput {
            occurred_at: now_rfc3339(),
            kind: EVENT_KIND_INTEGRITY_CHECK.to_string(),
            account_id: None,
            mailbox_id: None,
            message_blob_id: None,
            detail: r#"{"result":"ok"}"#.to_string(),
        });
        return;
    }

    // Record the tampering event with details.
    let issues_json: Vec<String> = status
        .issues
        .iter()
        .map(|i| format!(r#""{}""#, escape_json_value(i)))
        .collect();
    let _ = storage.append_event(&InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: EVENT_KIND_TAMPERING_DETECTED.to_string(),
        account_id: None,
        mailbox_id: None,
        message_blob_id: None,
        detail: format!(
            r#"{{"chain_ok":{},"root_hash_ok":{},"issues":[{}]}}"#,
            status.chain_ok,
            status.root_hash_ok,
            issues_json.join(","),
        ),
    });
}

/// Get the active database path from AppState.
fn get_active_db_path(app_handle: &AppHandle) -> Option<String> {
    let state = app_handle.state::<AppState>();
    let guard = state.active_db_path.lock().ok()?;
    guard.clone()
}

/// Query the most recent `occurred_at` for a given event kind.
fn get_last_event_time(storage: &Storage, kind: &str) -> Option<String> {
    storage.last_event_time_by_kind(kind).ok().flatten()
}

/// Get the system boot time as an RFC 3339 string.
/// Delegates to a platform-specific helper to obtain the boot timestamp.
fn get_system_boot_time_rfc3339() -> Option<String> {
    let secs = get_boot_time_unix_secs()?;
    let boot_time = time::OffsetDateTime::from_unix_timestamp(secs).ok()?;
    boot_time
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

/// macOS: parse `sysctl kern.boottime` → `{ sec = N, usec = ... }`.
#[cfg(target_os = "macos")]
fn get_boot_time_unix_secs() -> Option<i64> {
    let output = std::process::Command::new("sysctl")
        .args(["-n", "kern.boottime"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let sec_start = text.find("sec = ")? + 6;
    let sec_end = text[sec_start..].find(',')?;
    text[sec_start..sec_start + sec_end].trim().parse().ok()
}

/// Linux: parse `btime` line from `/proc/stat`.
#[cfg(target_os = "linux")]
fn get_boot_time_unix_secs() -> Option<i64> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    for line in stat.lines() {
        if let Some(rest) = line.strip_prefix("btime ") {
            return rest.trim().parse().ok();
        }
    }
    None
}

/// Windows: parse `wmic os get lastbootuptime` → `YYYYMMDDHHMMSS.ffffff+ZZZ`.
#[cfg(target_os = "windows")]
fn get_boot_time_unix_secs() -> Option<i64> {
    let output = std::process::Command::new("wmic")
        .args(["os", "get", "lastbootuptime"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Output has a header line, then the timestamp.
    let ts_line = text.lines().nth(1)?.trim();
    if ts_line.len() < 14 {
        return None;
    }
    // Parse YYYYMMDDHHMMSS
    let year: i32 = ts_line[0..4].parse().ok()?;
    let month: u8 = ts_line[4..6].parse().ok()?;
    let day: u8 = ts_line[6..8].parse().ok()?;
    let hour: u8 = ts_line[8..10].parse().ok()?;
    let minute: u8 = ts_line[10..12].parse().ok()?;
    let second: u8 = ts_line[12..14].parse().ok()?;

    let date =
        time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    let time_val = time::Time::from_hms(hour, minute, second).ok()?;
    let dt = time::PrimitiveDateTime::new(date, time_val).assume_utc();
    Some(dt.unix_timestamp())
}

fn parse_rfc3339(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}

/// Minimal JSON string escaping for values interpolated into detail JSON.
fn escape_json_value(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

pub async fn run_sync_all_accounts_once(app_handle: &AppHandle) -> Result<UiSyncStatus, String> {
    let state = app_handle.state::<AppState>();

    let db_path = {
        state
            .active_db_path
            .lock()
            .map_err(|_| "internal error: mutex poisoned".to_string())?
            .clone()
    };
    let Some(db_path) = db_path else {
        return Ok(UiSyncStatus {
            sync_in_progress: false,
            last_sync_at: None,
            last_sync_status: "not configured".to_string(),
        });
    };

    let _guard = state.sync_lock.lock().await;
    state.set_sync_in_progress(true);
    state.set_tray_status_text("Status: syncing…");
    let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

    let progress_handle = app_handle.clone();
    let on_progress: SyncProgressFn = Box::new(move |p| {
        let _ = progress_handle.emit(EVENT_SYNC_PROGRESS, p);
    });

    let now = now_rfc3339();
    let result = sync_all_accounts_inner(&db_path, &on_progress).await;

    let next = match &result {
        Ok(summary) => {
            let status = if summary.accounts_with_errors > 0 {
                "partial"
            } else {
                "ok"
            };
            let status_text = format_last_sync_status_text(status, now.as_str());

            UiSyncStatus {
                sync_in_progress: false,
                last_sync_at: Some(now.clone()),
                last_sync_status: status_text,
            }
        }
        Err(_err) => {
            let status_text = format_last_sync_status_text("error", now.as_str());
            UiSyncStatus {
                sync_in_progress: false,
                last_sync_at: Some(now.clone()),
                last_sync_status: status_text,
            }
        }
    };

    if let Ok(mut guard) = state.last_sync.lock() {
        *guard = next.clone();
    }

    state.set_sync_in_progress(false);
    state.set_tray_status_text(&format!("Last sync: {}", next.last_sync_status));

    let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

    result.map(|_summary| next)
}

struct AggregateSyncSummary {
    accounts_synced: usize,
    accounts_with_errors: usize,
    mailboxes_seen_total: usize,
    mailboxes_synced_total: usize,
    messages_fetched_total: u64,
    messages_ingested_total: u64,
}

async fn sync_all_accounts_inner(
    db_path: &str,
    on_progress: &SyncProgressFn,
) -> Result<AggregateSyncSummary, String> {
    let storage = Storage::open_or_create(db_path).map_err(|e| e.to_string())?;
    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    let secret_store = KeychainSecretStore::new();

    let mut aggregate = AggregateSyncSummary {
        accounts_synced: 0,
        accounts_with_errors: 0,
        mailboxes_seen_total: 0,
        mailboxes_synced_total: 0,
        messages_fetched_total: 0,
        messages_ingested_total: 0,
    };

    for account in accounts {
        if account.disabled {
            continue;
        }

        match sync_account_once_with_progress(&storage, &secret_store, &account, Some(on_progress))
            .await
        {
            Ok(summary) => {
                aggregate.accounts_synced += 1;
                aggregate.mailboxes_seen_total += summary.mailboxes_seen;
                aggregate.mailboxes_synced_total += summary.mailboxes_synced;
                aggregate.messages_fetched_total += summary.messages_fetched;
                aggregate.messages_ingested_total += summary.messages_ingested;
                if summary.had_mailbox_errors {
                    aggregate.accounts_with_errors += 1;
                }
            }
            Err(err) => {
                aggregate.accounts_with_errors += 1;
                let _ = err;
            }
        }
    }

    Ok(aggregate)
}

/// Run a periodic integrity check after a sync cycle.
///
/// - **Quick check (every cycle)**: compare the current root hash against the
///   checkpoint embedded in the latest `sync_finished` event.
/// - **Full check (every N cycles)**: additionally verify the entire event
///   hash chain.
///
/// Results are recorded as `integrity_check` events and any anomaly triggers
/// a `tampering_detected` event.
fn run_periodic_integrity_check(app_handle: &AppHandle, run_full_chain: bool) {
    let Some(db_path) = get_active_db_path(app_handle) else {
        return;
    };
    let Ok(storage) = Storage::open_or_create(&db_path) else {
        return;
    };

    let status = if run_full_chain {
        // Full verification (chain + root hash).
        match storage.verify_integrity() {
            Ok(s) => s,
            Err(_) => return,
        }
    } else {
        // Quick root-hash-only check.
        match storage.verify_root_hash_only() {
            Ok(s) => s,
            Err(_) => return,
        }
    };

    // Update the app state so the UI reflects the latest status.
    let state = app_handle.state::<AppState>();
    if let Ok(mut guard) = state.integrity_status.lock() {
        *guard = Some(status.clone());
    }

    let check_kind = if run_full_chain { "full" } else { "quick" };

    if status.ok {
        let _ = storage.append_event(&InsertEventInput {
            occurred_at: now_rfc3339(),
            kind: EVENT_KIND_INTEGRITY_CHECK.to_string(),
            account_id: None,
            mailbox_id: None,
            message_blob_id: None,
            detail: format!(r#"{{"result":"ok","kind":"{}"}}"#, check_kind),
        });
        return;
    }

    // Record tampering.
    let issues_json: Vec<String> = status
        .issues
        .iter()
        .map(|i| format!(r#""{}""#, escape_json_value(i)))
        .collect();
    let _ = storage.append_event(&InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: EVENT_KIND_TAMPERING_DETECTED.to_string(),
        account_id: None,
        mailbox_id: None,
        message_blob_id: None,
        detail: format!(
            r#"{{"kind":"{}","chain_ok":{},"root_hash_ok":{},"issues":[{}]}}"#,
            check_kind,
            status.chain_ok,
            status.root_hash_ok,
            issues_json.join(","),
        ),
    });
}

fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
