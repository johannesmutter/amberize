use std::path::{Path, PathBuf};
use std::process::Command;

use email_archiver_storage::Storage;
use thiserror::Error;

const VERFAHRENSDOKUMENTATION_FILENAME: &str = "verfahrensdokumentation.md";
const EVENT_KIND_DOCUMENTATION_GENERATED: &str = "documentation_generated";

const AUTO_BEGIN_MARKER: &str = "<!-- BEGIN AUTO-GENERATED TECHNISCHE_SYSTEMDOKUMENTATION -->";
const AUTO_END_MARKER: &str = "<!-- END AUTO-GENERATED TECHNISCHE_SYSTEMDOKUMENTATION -->";

const TEMPLATE_DE: &str = include_str!("../assets/verfahrensdokumentation_template_de.md");

#[derive(Debug, Error)]
pub enum DocumentationError {
    #[error("storage error: {0}")]
    Storage(#[from] email_archiver_storage::StorageError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("template missing auto markers")]
    TemplateMissingMarkers,

    #[error("failed to open documentation file")]
    OpenFailed,
}

pub type DocumentationResult<T> = Result<T, DocumentationError>;

#[tauri::command]
pub async fn generate_documentation(db_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let documentation_path = ensure_verfahrensdokumentation(&storage, Path::new(&db_path))
            .map_err(|e| e.to_string())?;
        Ok(documentation_path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_documentation(db_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let documentation_path = ensure_verfahrensdokumentation(&storage, Path::new(&db_path))
            .map_err(|e| e.to_string())?;
        open_in_default_app(&documentation_path).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn ensure_verfahrensdokumentation(
    storage: &Storage,
    archive_path: &Path,
) -> DocumentationResult<PathBuf> {
    let documentation_path = documentation_path_for_archive(archive_path)?;

    let updated_text = render_snapshot_documentation(storage, archive_path)?;

    crate::output::atomic_export(archive_path, &documentation_path, |file| {
        use std::io::Write;
        file.write_all(updated_text.as_bytes())
            .map_err(|e| e.to_string())
    })
    .map_err(std::io::Error::other)?;

    // Audit-relevant: documentation generation/refresh.
    let _ = storage.append_event(&email_archiver_storage::InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: EVENT_KIND_DOCUMENTATION_GENERATED.to_string(),
        account_id: None,
        mailbox_id: None,
        message_blob_id: None,
        detail: r#"{"v":1}"#.to_string(),
    });

    Ok(documentation_path)
}

fn documentation_path_for_archive(archive_path: &Path) -> DocumentationResult<PathBuf> {
    let parent = archive_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "archive path has no parent",
        )
    })?;
    let name = archive_path
        .file_name()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "Archive filename missing")
        })?
        .to_string_lossy();
    Ok(parent.join(format!("{name}.{VERFAHRENSDOKUMENTATION_FILENAME}")))
}

fn render_auto_technical_section(
    storage: &Storage,
    archive_path: &Path,
    check: &impl Fn() -> email_archiver_storage::StorageResult<()>,
) -> DocumentationResult<String> {
    check()?;
    let schema_version = storage.schema_version()?;
    let proof_snapshot = storage.create_proof_snapshot_checked(check)?;
    let event_chain = storage.verify_event_chain_checked(check)?;

    let accounts = storage.list_accounts()?;

    let mut lines = Vec::new();
    lines.push(AUTO_BEGIN_MARKER.to_string());
    lines.push(String::new());
    lines.push("### Technische Systemdokumentation (automatisch generiert)".to_string());
    lines.push(String::new());

    lines.push("**Software**".to_string());
    lines.push("- Produkt: Amberize".to_string());
    lines.push(format!("- Version: {}", env!("CARGO_PKG_VERSION")));
    lines.push(format!("- Plattform: {}", current_platform_name()));
    lines.push(String::new());

    lines.push("**Archiv-Speicherort**".to_string());
    lines.push(format!(
        "- SQLite-Datei: `{}`",
        archive_path.to_string_lossy()
    ));
    lines.push(String::new());

    lines.push("**Synchronisation (IMAP)**".to_string());
    lines.push(format!(
        "- Konfiguriertes Intervall: {} Minuten",
        storage.sync_interval_secs()? / 60
    ));
    lines.push("- IMAP Flags werden nicht verändert (BODY.PEEK[]).".to_string());
    lines.push("- Im Automatikmodus werden alle auswählbaren Ordner archiviert. Im manuellen Modus gilt die gespeicherte Ordnerauswahl.".to_string());
    lines.push(String::new());

    lines.push("**Konfiguration (ohne Geheimnisse)**".to_string());
    if accounts.is_empty() {
        lines.push("- (keine Konten konfiguriert)".to_string());
    } else {
        for account in &accounts {
            lines.push(format!(
                "- Konto #{}: {} ({}, {}:{}, TLS={})",
                account.id,
                account.label,
                account.email_address,
                account.imap_host,
                account.imap_port,
                if account.disabled {
                    "Archiv bleibt sichtbar; Synchronisation deaktiviert"
                } else if account.imap_tls {
                    "ja"
                } else {
                    "nein"
                }
            ));

            let mailboxes = storage.list_mailboxes(account.id)?;
            let included = mailboxes
                .iter()
                .filter(|m| m.sync_enabled && !m.hard_excluded)
                .map(|m| m.imap_name.as_str())
                .collect::<Vec<_>>();
            let excluded = mailboxes
                .iter()
                .filter(|m| !m.sync_enabled || m.hard_excluded)
                .map(|m| m.imap_name.as_str())
                .collect::<Vec<_>>();

            if !included.is_empty() {
                lines.push(format!("  - Archivierte Ordner: {}", included.join(", ")));
            }
            if !excluded.is_empty() {
                lines.push(format!(
                    "  - Nicht archivierte Ordner: {}",
                    excluded.join(", ")
                ));
            }
        }
    }
    lines.push(String::new());

    lines.push("**Datenbank / Schema**".to_string());
    lines.push(format!("- Schema-Version: {}", schema_version));
    lines.push(
        "- Tabellen: accounts, mailboxes, message_blobs, message_locations, events, messages_fts"
            .to_string(),
    );
    lines.push("- FTS5: Volltextsuche über Betreff und extrahierten Text".to_string());
    lines.push(String::new());

    lines.push("**Integrität & Nachvollziehbarkeit (tamper-evidence)**".to_string());
    lines.push("- Jede archivierte Nachricht wird als Originalbytefolge gespeichert.".to_string());
    lines.push(
        "- Für jede Nachricht wird ein SHA-256 Hash gespeichert (message_blobs.sha256)."
            .to_string(),
    );
    lines.push("- Event-Log ist hash-verkettet (prev_hash → hash).".to_string());
    lines.push("- Inhaltsprüfungen erkennen Abweichungen von gespeicherten Hashes. Ohne extern gesicherten Prüfsnapshot kann eine koordinierte Neuschreibung des gesamten Archivs nicht bewiesen werden.".to_string());
    lines.push(format!(
        "- Event-Chain-Check: geprüft={}, erster Fehler={}",
        event_chain.checked_events,
        event_chain
            .first_mismatch_event_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "kein".to_string())
    ));
    lines.push(String::new());

    lines.push("**Proof Snapshot**".to_string());
    lines.push(format!("- Zeitpunkt: {}", proof_snapshot.created_at));
    lines.push(format!(
        "- Letztes Event: id={}, hash={}",
        proof_snapshot
            .last_event_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "—".to_string()),
        proof_snapshot
            .last_event_hash
            .clone()
            .unwrap_or_else(|| "—".to_string())
    ));
    lines.push(format!(
        "- Counts: accounts={}, mailboxes={}, blobs={}, locations={}, events={}",
        proof_snapshot.accounts_count,
        proof_snapshot.mailboxes_count,
        proof_snapshot.message_blobs_count,
        proof_snapshot.message_locations_count,
        proof_snapshot.events_count
    ));
    lines.push(format!(
        "- Root-Hash (message_blobs.sha256): {}",
        proof_snapshot.message_blobs_root_hash
    ));
    lines.push(String::new());

    lines.push(AUTO_END_MARKER.to_string());
    lines.push(String::new());

    Ok(lines.join("\n"))
}

fn replace_between_markers(base_text: &str, auto_block: &str) -> DocumentationResult<String> {
    let begin_index = base_text
        .find(AUTO_BEGIN_MARKER)
        .ok_or(DocumentationError::TemplateMissingMarkers)?;
    let end_index = base_text
        .find(AUTO_END_MARKER)
        .ok_or(DocumentationError::TemplateMissingMarkers)?;

    if end_index < begin_index {
        return Err(DocumentationError::TemplateMissingMarkers);
    }

    let end_index_inclusive = end_index + AUTO_END_MARKER.len();

    let mut updated = String::new();
    updated.push_str(&base_text[..begin_index]);
    updated.push_str(auto_block);
    updated.push_str(&base_text[end_index_inclusive..]);
    Ok(updated)
}

fn open_in_default_app(path: &Path) -> DocumentationResult<()> {
    // Validate the path doesn't contain traversal sequences.
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(DocumentationError::OpenFailed);
        }
    }

    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(path).status()?;

    #[cfg(target_os = "windows")]
    let status = Command::new("cmd")
        .args(["/c", "start", ""])
        .arg(path)
        .status()?;

    #[cfg(target_os = "linux")]
    let status = Command::new("xdg-open").arg(path).status()?;

    if !status.success() {
        return Err(DocumentationError::OpenFailed);
    }

    Ok(())
}

/// Return a human-readable platform name for the generated documentation.
fn current_platform_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub fn render_snapshot_documentation(
    storage: &Storage,
    archive_path: &Path,
) -> DocumentationResult<String> {
    render_snapshot_documentation_checked(storage, archive_path, || Ok(()))
}

pub fn render_snapshot_documentation_checked(
    storage: &Storage,
    archive_path: &Path,
    check: impl Fn() -> email_archiver_storage::StorageResult<()>,
) -> DocumentationResult<String> {
    check()?;
    let path = documentation_path_for_archive(archive_path)?;
    let legacy = archive_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(VERFAHRENSDOKUMENTATION_FILENAME);
    let base = if path.exists() {
        std::fs::read_to_string(path)?
    } else if legacy.exists() {
        let text = std::fs::read_to_string(legacy)?;
        if text.contains(&format!("`{}`", archive_path.display())) {
            text
        } else {
            TEMPLATE_DE.into()
        }
    } else {
        TEMPLATE_DE.into()
    };
    replace_between_markers(
        &base,
        &render_auto_technical_section(storage, archive_path, &check)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_between_markers_replaces_content() {
        let base = format!("a\n{AUTO_BEGIN_MARKER}\nold\n{AUTO_END_MARKER}\nb\n");
        let auto = format!("{AUTO_BEGIN_MARKER}\nnew\n{AUTO_END_MARKER}\n");
        let updated = replace_between_markers(&base, &auto).unwrap();
        assert!(updated.contains("new"));
        assert!(!updated.contains("old"));
    }
}
