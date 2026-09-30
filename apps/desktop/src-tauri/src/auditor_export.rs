use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use email_archiver_storage::{IntegrityCheckResult, Storage};
use serde::Serialize;
use thiserror::Error;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::app_state::AppState;
use crate::documentation::{render_snapshot_documentation_checked, DocumentationError};
use crate::output::{check_export, ExportState};

const EVENT_KIND_AUDITOR_EXPORT: &str = "auditor_export";

const INTEGRITY_MAX_MISMATCHES: usize = 100;

#[derive(Debug, Error)]
pub enum AuditorExportError {
    #[error("storage error: {0}")]
    Storage(#[from] email_archiver_storage::StorageError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("documentation error: {0}")]
    Documentation(#[from] DocumentationError),
}

pub type AuditorExportResult<T> = Result<T, AuditorExportError>;

pub struct ExportOperation {
    state: Arc<Mutex<ExportState>>,
    registered: bool,
}

fn prune_operations(operations: &mut std::collections::HashMap<String, ExportOperation>) {
    if operations.len() >= 128 {
        operations.retain(|_, op| {
            !op.registered
                || Arc::strong_count(&op.state) > 1
                || op
                    .state
                    .lock()
                    .map(|s| *s == ExportState::Running)
                    .unwrap_or(true)
        });
    }
}

fn register_export(state: &AppState, id: String) -> Result<Arc<Mutex<ExportState>>, String> {
    uuid::Uuid::parse_str(&id).map_err(|_| "Invalid export operation ID")?;
    let mut operations = state
        .export_operations
        .lock()
        .map_err(|_| "Export state unavailable")?;
    if let Some(op) = operations.get_mut(&id) {
        if !op.registered {
            op.registered = true;
            return Err("Export canceled. Your previous file was kept.".into());
        }
        return Err("This export operation has already started".into());
    }
    prune_operations(&mut operations);
    if operations.len() >= 128
        || operations
            .values()
            .filter(|op| {
                op.state
                    .lock()
                    .map(|s| *s == ExportState::Running)
                    .unwrap_or(true)
            })
            .count()
            >= 4
    {
        return Err("Too many pending exports. Wait for an export to finish.".into());
    }
    let operation = Arc::new(Mutex::new(ExportState::Running));
    operations.insert(
        id,
        ExportOperation {
            state: operation.clone(),
            registered: true,
        },
    );
    Ok(operation)
}

fn request_cancel(state: &AppState, id: String) -> Result<bool, String> {
    uuid::Uuid::parse_str(&id).map_err(|_| "Invalid export operation ID")?;
    let mut operations = state
        .export_operations
        .lock()
        .map_err(|_| "Export state unavailable")?;
    if let Some(op) = operations.get(&id) {
        let operation = op.state.clone();
        drop(operations);
        return crate::output::cancel_export(&operation);
    }
    prune_operations(&mut operations);
    if operations.len() >= 128 {
        return Err("Too many pending exports. Try canceling again shortly.".into());
    }
    // Native commands can be scheduled out of order; remember an early Cancel.
    operations.insert(
        id,
        ExportOperation {
            state: Arc::new(Mutex::new(ExportState::Cancelled)),
            registered: false,
        },
    );
    Ok(false)
}

#[tauri::command]
pub async fn cancel_auditor_export(
    app: tauri::AppHandle,
    operation_id: String,
) -> Result<bool, String> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        request_cancel(&app.state::<AppState>(), operation_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_auditor_package(
    state: tauri::State<'_, AppState>,
    db_path: String,
    output_zip_path: String,
    selected_blob_ids: Option<Vec<i64>>,
    operation_id: Option<String>,
) -> Result<String, String> {
    let operation = register_export(
        &state,
        operation_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
    )?;
    let worker = operation.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        check_export(Some(&worker)).map_err(|e| e.to_string())?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;

        let output_zip_path = PathBuf::from(output_zip_path);
        export_package(
            &storage,
            Path::new(&db_path),
            &output_zip_path,
            selected_blob_ids.as_deref(),
            Some(&worker),
        )
        .map_err(|e| e.to_string())?;

        Ok(output_zip_path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|result| result);
    if let Ok(mut status) = operation.lock() {
        if *status == ExportState::Running {
            *status = ExportState::Failed;
        }
    }
    result
}

#[cfg(test)]
pub fn export_auditor_package_to_path(
    storage: &Storage,
    archive_path: &Path,
    output_zip_path: &Path,
) -> AuditorExportResult<()> {
    export_package(storage, archive_path, output_zip_path, None, None)
}

fn export_package(
    storage: &Storage,
    archive_path: &Path,
    output_zip_path: &Path,
    selected: Option<&[i64]>,
    operation: Option<&Mutex<ExportState>>,
) -> AuditorExportResult<()> {
    let check = || check_export(operation).map_err(email_archiver_storage::StorageError::from);
    check()?;
    crate::output::validate_destination(archive_path, output_zip_path)?;
    let selected = selected.map(|ids| {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        ids
    });
    if let Some(ids) = &selected {
        if ids.is_empty() || ids.len() > 10000 || ids.iter().any(|id| *id <= 0) {
            return Err(std::io::Error::other("Select between 1 and 10000 valid messages").into());
        }
    }
    let dir = tempfile::tempdir()?;
    let snapshot = storage.snapshot_to_checked(&dir.path().join("snapshot.db"), check)?;
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    crate::output::atomic_export_cancellable(
        archive_path,
        output_zip_path,
        |file| {
            let write_result = (|| -> AuditorExportResult<()> {
                let mut zip = ZipWriter::new(file);
                if let Some(ids) = &selected {
                    // A selected export excludes unrelated accounts, locations,
                    // audit events and whole-archive proofs.
                    zip.start_file("manifest.jsonl", options)?;
                    snapshot.visit_raw_blobs(Some(ids), |raw| {
                        check()?;
                        serde_json::to_writer(
                            &mut zip,
                            &serde_json::json!({
                                "id": raw.id, "sha256": raw.sha256,
                                "eml_path": format!("messages/{}.eml", raw.sha256)
                            }),
                        )
                        .map_err(std::io::Error::other)?;
                        zip.write_all(b"\n")?;
                        Ok(())
                    })?;
                } else {
                    let proof = snapshot.create_proof_snapshot_checked(check)?;
                    let report = IntegrityReport {
                        created_at: proof.created_at.clone(),
                        event_chain: snapshot.verify_event_chain_checked(check)?,
                        message_blobs: snapshot.verify_message_blobs_integrity_checked(
                            INTEGRITY_MAX_MISMATCHES,
                            check,
                        )?,
                    };
                    zip.start_file("index.csv", options)?;
                    zip.write_all(build_index_csv(&[]).as_bytes())?;
                    snapshot.visit_auditor_index_rows(|row| {
                        check()?;
                        let text = build_index_csv(std::slice::from_ref(row));
                        let line = text.split_once('\n').unwrap().1;
                        zip.write_all(line.as_bytes())?;
                        Ok(())
                    })?;
                    zip.start_file("events.jsonl", options)?;
                    snapshot.visit_events_for_export(|row| {
                        check()?;
                        let line = build_events_jsonl(std::slice::from_ref(row))
                            .map_err(|e| std::io::Error::other(e.to_string()))?;
                        zip.write_all(line.as_bytes())?;
                        Ok(())
                    })?;
                    write_zip_text(
                        &mut zip,
                        "proof_snapshot.json",
                        &serde_json::to_string_pretty(&proof)?,
                        options,
                    )?;
                    write_zip_text(
                        &mut zip,
                        "integrity_report.json",
                        &serde_json::to_string_pretty(&report)?,
                        options,
                    )?;
                    write_zip_text(
                        &mut zip,
                        "verfahrensdokumentation.md",
                        &render_snapshot_documentation_checked(&snapshot, archive_path, check)?,
                        options,
                    )?;
                }
                snapshot.visit_raw_blobs(selected.as_deref(), |raw| {
                    check()?;
                    zip.start_file(format!("messages/{}.eml", raw.sha256), options)
                        .map_err(std::io::Error::other)?;
                    zip.write_all(&raw.raw_mime)?;
                    Ok(())
                })?;
                check()?;
                zip.finish()?;
                Ok(())
            })();
            write_result.map_err(|e| e.to_string())
        },
        operation,
    )
    .map_err(std::io::Error::other)?;
    storage.append_event(&email_archiver_storage::InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: if selected.is_some() {
            "selected_messages_exported"
        } else {
            EVENT_KIND_AUDITOR_EXPORT
        }
        .into(),
        account_id: None,
        mailbox_id: None,
        message_blob_id: None,
        detail: serde_json::json!({"v":2,"selected_count":selected.as_ref().map(Vec::len)})
            .to_string(),
    })?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct IntegrityReport {
    created_at: String,
    event_chain: email_archiver_storage::EventChainCheckResult,
    message_blobs: IntegrityCheckResult,
}

fn build_index_csv(rows: &[email_archiver_storage::AuditorIndexRow]) -> String {
    let headers = [
        "account_id",
        "account_label",
        "mailbox_name",
        "uidvalidity",
        "uid",
        "internal_date",
        "flags",
        "message_blob_id",
        "sha256",
        "message_id",
        "date_header",
        "from_address",
        "to_addresses",
        "cc_addresses",
        "subject",
        "imported_at",
        "eml_path",
    ];

    let mut lines = Vec::new();
    lines.push(headers.join(","));

    for row in rows {
        let eml_path = format!("messages/{}.eml", row.sha256);
        let fields = [
            row.account_id.to_string(),
            row.account_label.clone(),
            row.mailbox_name.clone(),
            row.uidvalidity.to_string(),
            row.uid.to_string(),
            row.internal_date.clone().unwrap_or_default(),
            row.flags.clone().unwrap_or_default(),
            row.message_blob_id.to_string(),
            row.sha256.clone(),
            row.message_id.clone().unwrap_or_default(),
            row.date_header.clone().unwrap_or_default(),
            row.from_address.clone().unwrap_or_default(),
            row.to_addresses.clone().unwrap_or_default(),
            row.cc_addresses.clone().unwrap_or_default(),
            row.subject.clone().unwrap_or_default(),
            row.imported_at.clone(),
            eml_path,
        ];

        lines.push(
            fields
                .iter()
                .map(|f| csv_escape(f))
                .collect::<Vec<_>>()
                .join(","),
        );
    }

    lines.join("\n") + "\n"
}

fn build_events_jsonl(
    rows: &[email_archiver_storage::EventExportRow],
) -> AuditorExportResult<String> {
    #[derive(Debug, Serialize)]
    struct EventLine<'a> {
        id: i64,
        occurred_at: &'a str,
        kind: &'a str,
        account_id: Option<i64>,
        mailbox_id: Option<i64>,
        message_blob_id: Option<i64>,
        detail: Option<&'a str>,
        prev_hash: &'a str,
        hash: &'a str,
    }

    let mut out = String::new();
    for row in rows {
        let line = EventLine {
            id: row.id,
            occurred_at: row.occurred_at.as_str(),
            kind: row.kind.as_str(),
            account_id: row.account_id,
            mailbox_id: row.mailbox_id,
            message_blob_id: row.message_blob_id,
            detail: row.detail.as_deref(),
            prev_hash: row.prev_hash.as_str(),
            hash: row.hash.as_str(),
        };

        out.push_str(&serde_json::to_string(&line)?);
        out.push('\n');
    }

    Ok(out)
}

fn write_zip_text<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    path: &str,
    text: &str,
    options: SimpleFileOptions,
) -> AuditorExportResult<()> {
    zip.start_file(path, options)?;
    zip.write_all(text.as_bytes())?;
    Ok(())
}

fn csv_escape(value: &str) -> String {
    let must_quote =
        value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r');
    if !must_quote {
        return value.to_string();
    }

    let escaped = value.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_fixture(path: &Path) -> Storage {
        let storage = Storage::create_new(path).unwrap();
        // Known SHA-256 fixtures, computed independently by Python hashlib.
        for (bytes, hash) in [
            (
                b"one".as_slice(),
                "7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed",
            ),
            (
                b"two".as_slice(),
                "3fc4ccfe745870e2c0d99f71f30ff0656c8dedd41cc1d7d3d376b0dbe685e2f3",
            ),
        ] {
            storage
                .insert_message_blob_if_absent(
                    &email_archiver_storage::InsertMessageBlobInput::raw(
                        hash.into(),
                        bytes.to_vec(),
                        "2026-01-01T00:00:00Z".into(),
                        email_archiver_storage::MessageBlobMetadata::default(),
                    ),
                )
                .unwrap();
        }
        storage
    }
    #[test]
    fn selected_export_contains_only_the_selection_and_no_unrelated_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.db");
        let storage = archive_fixture(&path);
        let output = dir.path().join("selected.zip");
        export_package(&storage, &path, &output, Some(&[1, 1]), None).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        assert_eq!(zip.len(), 2);
        assert!(zip.by_name("events.jsonl").is_err());
        let mut bytes = vec![];
        use std::io::Read;
        zip.by_name(
            "messages/7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed.eml",
        )
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
        assert_eq!(bytes, b"one");
    }
    #[test]
    fn cancelled_package_keeps_previous_output_and_adds_no_export_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.db");
        let storage = archive_fixture(&path);
        let output = dir.path().join("export.zip");
        std::fs::write(&output, b"previous").unwrap();
        let state = Mutex::new(ExportState::Cancelled);
        let result = export_package(&storage, &path, &output, None, Some(&state));
        assert!(result.unwrap_err().to_string().contains("canceled"));
        assert_eq!(std::fs::read(output).unwrap(), b"previous");
        assert_eq!(storage.event_count(None).unwrap(), 0);
    }
    #[test]
    fn early_cancellation_blocks_a_late_start_and_export_registry_is_bounded() {
        let state = AppState::default();
        let id = uuid::Uuid::new_v4().to_string();
        assert!(!request_cancel(&state, id.clone()).unwrap());
        assert!(register_export(&state, id)
            .err()
            .unwrap()
            .contains("canceled"));
        let running: Vec<_> = (0..4)
            .map(|_| register_export(&state, uuid::Uuid::new_v4().to_string()).unwrap())
            .collect();
        assert!(register_export(&state, uuid::Uuid::new_v4().to_string()).is_err());
        for op in running {
            *op.lock().unwrap() = ExportState::Failed;
        }
        for _ in 0..300 {
            let op = register_export(&state, uuid::Uuid::new_v4().to_string()).unwrap();
            *op.lock().unwrap() = ExportState::Published;
        }
        assert!(state.export_operations.lock().unwrap().len() <= 128);
    }
    #[test]
    fn same_archive_path_is_rejected_and_invalid_selection_keeps_previous_export() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.db");
        let storage = archive_fixture(&path);
        let before = std::fs::read(&path).unwrap();
        assert!(export_auditor_package_to_path(&storage, &path, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let output = dir.path().join("previous.zip");
        std::fs::write(&output, b"previous").unwrap();
        assert!(export_package(&storage, &path, &output, Some(&[999]), None).is_err());
        assert_eq!(std::fs::read(output).unwrap(), b"previous");
    }
    #[test]
    fn full_export_proof_events_and_content_share_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.db");
        let storage = archive_fixture(&path);
        let output = dir.path().join("full.zip");
        export_auditor_package_to_path(&storage, &path, &output).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        use std::io::Read;
        let mut proof = String::new();
        zip.by_name("proof_snapshot.json")
            .unwrap()
            .read_to_string(&mut proof)
            .unwrap();
        let proof: serde_json::Value = serde_json::from_str(&proof).unwrap();
        assert_eq!(proof["message_blobs_count"], 2);
        assert_eq!(proof["events_count"], 0);
        let mut events = String::new();
        zip.by_name("events.jsonl")
            .unwrap()
            .read_to_string(&mut events)
            .unwrap();
        assert!(events.is_empty());
        assert_eq!(storage.event_count(None).unwrap(), 1);
    }
    #[test]
    fn csv_escape_quotes_when_needed() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
    }
}
