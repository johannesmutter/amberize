//! Compiled only against the v0.2.3 storage library in a disposable export.
use email_archiver_storage::{
    CreateAccountInput, IngestMessageLocationInput, InsertMessageBlobInput, MessageBlobMetadata,
    Storage, UpsertMailboxInput,
};
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(2)
        .ok_or("Usage: old_fixture create|verify PATH [COUNT]")?;
    let create = args.get(1).map(String::as_str) == Some("create");
    if create == std::path::Path::new(path).exists() {
        return Err("Create requires a new archive; verify requires an existing archive".into());
    }
    let storage = Storage::open_or_create(path)?;
    let mut hashes = vec![];
    if create {
        let count: u32 = args.get(3).map(String::as_str).unwrap_or("1024").parse()?;
        let account = storage.create_account(&CreateAccountInput::classic_imap_password(
            "Synthetic old-version QA mailbox".into(),
            "qa@example.invalid".into(),
            "127.0.0.1".into(),
            9,
            true,
            "qa@example.invalid".into(),
            format!("qa-old-fixture/{}", std::process::id()),
        ))?;
        let mailbox = storage.upsert_mailbox(&UpsertMailboxInput {
            account_id: account,
            imap_name: "INBOX".into(),
            delimiter: Some("/".into()),
            attributes: None,
            sync_enabled: false,
            hard_excluded: false,
            uidvalidity: Some(1),
            last_seen_uid: count,
        })?;
        for uid in 1..=count {
            let date = format!("2026-09-{:02}T12:00:00Z", 1 + uid % 28);
            let raw = format!("Message-ID: <old-qa-{uid}@example.invalid>\r\nFrom: qa@example.invalid\r\nTo: recipient@example.invalid\r\nSubject: Old-version QA message {uid}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSynthetic old-version message. No private data.\r\n").into_bytes();
            let hash = hex::encode(Sha256::digest(&raw));
            hashes.push(hash.clone());
            let blob = InsertMessageBlobInput::raw(
                hash,
                raw,
                date.clone(),
                MessageBlobMetadata {
                    message_id: Some(format!("<old-qa-{uid}@example.invalid>")),
                    date_header: Some(date.clone()),
                    from_address: Some("qa@example.invalid".into()),
                    to_addresses: Some("recipient@example.invalid".into()),
                    subject: Some(format!("Old-version QA message {uid}")),
                    body_text: Some("Synthetic old-version message. No private data.".into()),
                    ..Default::default()
                },
            );
            storage.ingest_message(
                &blob,
                &IngestMessageLocationInput {
                    account_id: account,
                    mailbox_id: mailbox,
                    uidvalidity: 1,
                    uid,
                    internal_date: Some(date.clone()),
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: date.clone(),
                    last_seen_at: date,
                },
            )?;
        }
    }
    let integrity = storage.verify_integrity()?;
    if !integrity.ok {
        return Err("Older archive failed full integrity verification".into());
    }
    hashes.sort();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "integrity":integrity, "mime_hashes":hashes, "created_with_old_storage":create,
        }))?
    );
    Ok(())
}
