use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use email_archiver_storage::{IntegrityCheckResult, Storage};
use serde::Serialize;
use thiserror::Error;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::documentation::{ensure_verfahrensdokumentation, DocumentationError};

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

#[tauri::command]
pub fn export_auditor_package(db_path: String, output_zip_path: String) -> Result<String, String> {
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;

    let output_zip_path = PathBuf::from(output_zip_path);
    export_auditor_package_to_path(&storage, Path::new(&db_path), &output_zip_path)
        .map_err(|e| e.to_string())?;

    Ok(output_zip_path.to_string_lossy().to_string())
}

pub fn export_auditor_package_to_path(
    storage: &Storage,
    archive_path: &Path,
    output_zip_path: &Path,
) -> AuditorExportResult<()> {
    create_parent_dir_if_needed(output_zip_path)?;

    let documentation_path = ensure_verfahrensdokumentation(storage, archive_path)?;
    let documentation_text = std::fs::read_to_string(&documentation_path)?;

    let proof_snapshot = storage.create_proof_snapshot()?;
    let event_chain = storage.verify_event_chain()?;
    let message_blob_integrity =
        storage.verify_message_blobs_integrity(INTEGRITY_MAX_MISMATCHES)?;

    let index_rows = storage.list_auditor_index_rows()?;
    let index_csv = build_index_csv(&index_rows);

    let events = storage.list_events_for_export()?;
    let events_jsonl = build_events_jsonl(&events)?;

    let integrity_report = IntegrityReport {
        created_at: proof_snapshot.created_at.clone(),
        event_chain,
        message_blobs: message_blob_integrity,
    };

    let proof_snapshot_json = serde_json::to_string_pretty(&proof_snapshot)?;
    let integrity_report_json = serde_json::to_string_pretty(&integrity_report)?;

    let file = File::create(output_zip_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    write_zip_text(&mut zip, "index.csv", &index_csv, options)?;
    write_zip_text(&mut zip, "events.jsonl", &events_jsonl, options)?;
    write_zip_text(
        &mut zip,
        "proof_snapshot.json",
        &proof_snapshot_json,
        options,
    )?;
    write_zip_text(
        &mut zip,
        "integrity_report.json",
        &integrity_report_json,
        options,
    )?;
    write_zip_text(
        &mut zip,
        "verfahrensdokumentation.md",
        &documentation_text,
        options,
    )?;

    // Store each message blob exactly once by content hash.
    let blobs = storage.list_message_blobs_for_export()?;
    for blob in blobs {
        let raw = storage.get_message_blob_raw_mime(blob.id)?;
        let zip_path = format!("messages/{}.eml", raw.sha256);
        zip.start_file(zip_path, options)?;
        zip.write_all(&raw.raw_mime)?;
    }

    zip.finish()?;

    // Audit-relevant: export event.
    let _ = storage.append_event(&email_archiver_storage::InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: EVENT_KIND_AUDITOR_EXPORT.to_string(),
        account_id: None,
        mailbox_id: None,
        message_blob_id: None,
        detail: r#"{"v":1}"#.to_string(),
    });

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

fn create_parent_dir_if_needed(path: &Path) -> AuditorExportResult<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(parent)?;
    Ok(())
}

fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_escape_quotes_when_needed() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
    }
}
