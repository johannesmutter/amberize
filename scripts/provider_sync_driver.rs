//! Copied into the exact candidate export for disposable loopback TLS integration QA.
use email_archiver_adapters::{sync_account_once, MemorySecretStore, SecretStore};
use email_archiver_storage::{CreateAccountInput, Storage};
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        return Err("Provider QA requires a fresh CI profile".into());
    }
    let args: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(args.get(2).ok_or("Missing archive")?);
    let root = PathBuf::from(std::env::var_os("RUNNER_TEMP").ok_or("Missing RUNNER_TEMP")?);
    if !path.starts_with(root) {
        return Err("Provider QA archive must stay in RUNNER_TEMP".into());
    }
    let storage = if args.get(1).map(String::as_str) == Some("create") {
        let port: u16 = args.get(3).ok_or("Missing loopback port")?.parse()?;
        if port < 1024 {
            return Err("Provider QA requires an unprivileged loopback port".into());
        }
        let storage = Storage::create_new(&path)?;
        storage.create_account(&CreateAccountInput::classic_imap_password(
            "Synthetic TLS protocol QA".into(),
            "qa@example.invalid".into(),
            "127.0.0.1".into(),
            port,
            true,
            "qa@example.invalid".into(),
            format!("qa-loopback/{}", std::process::id()),
        ))?;
        storage
    } else {
        Storage::open_existing(&path)?
    };
    let accounts = storage.list_accounts()?;
    if accounts.len() != 1
        || accounts[0].imap_host != "127.0.0.1"
        || !accounts[0].secret_ref.starts_with("qa-loopback/")
    {
        return Err("Provider QA refuses non-synthetic accounts".into());
    }
    let mut status = "created";
    let mut imported = 0;
    let mut errors = Vec::new();
    if args.get(1).map(String::as_str) == Some("sync") {
        let secrets = MemorySecretStore::new();
        secrets.set_secret(&accounts[0].secret_ref, "amberize-synthetic-protocol-qa")?;
        match sync_account_once(&storage, &secrets, &accounts[0]).await {
            Ok(summary) => {
                status = if summary.had_mailbox_errors {
                    "partial"
                } else {
                    "ok"
                };
                imported = summary.messages_ingested;
                errors = summary.errors;
            }
            Err(error) => {
                status = "error";
                errors.push(error.to_string());
            }
        }
    } else if args.get(1).map(String::as_str) != Some("create") {
        return Err("Usage: provider_sync_qa create|sync ARCHIVE [PORT]".into());
    }
    let integrity = storage.verify_integrity()?;
    let proof = storage.create_proof_snapshot()?;
    let mailboxes: Vec<_> = storage
        .list_mailboxes(accounts[0].id)?
        .into_iter()
        .map(|mailbox| {
            serde_json::json!({"name": mailbox.imap_name, "generation": mailbox.uidvalidity,
            "cursor": mailbox.last_seen_uid, "error": mailbox.last_error})
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({"status": status, "messages_imported": imported,
        "errors": errors, "blob_count": proof.message_blobs_count,
        "location_count": proof.message_locations_count, "integrity_ok": integrity.ok,
        "mailboxes": mailboxes})
    );
    Ok(())
}
