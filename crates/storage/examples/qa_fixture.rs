//! Synthetic archive for pre-release checks. Never accepts an existing output archive.
use email_archiver_storage::{
    CreateAccountInput, IngestMessageLocationInput, InsertMessageBlobInput, MessageBlobMetadata,
    Storage, UpsertMailboxInput,
};
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(2)
        .ok_or("Usage: qa_fixture create|verify PATH [COUNT]")?;
    let storage = match args.get(1).map(String::as_str) {
        Some("create") => {
            let count: u32 = args.get(3).map(String::as_str).unwrap_or("1024").parse()?;
            let storage = Storage::create_new(path)?;
            let account = storage.create_account(&CreateAccountInput::classic_imap_password(
                "Synthetic QA mailbox".into(),
                "qa@example.invalid".into(),
                "127.0.0.1".into(),
                9,
                true,
                "qa@example.invalid".into(),
                format!("qa-fixture/{}", std::process::id()),
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
                let subject = format!("QA message {uid:06}");
                let headers = format!("Message-ID: <qa-{uid}@example.invalid>\r\nFrom: qa@example.invalid\r\nTo: recipient@example.invalid\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\n");
                let body = if uid % 16 == 0 {
                    "Content-Type: multipart/mixed; boundary=qa-boundary\r\n\r\n--qa-boundary\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Synthetic QA message</p><img src=\"https://example.invalid/tracker.png\">\r\n--qa-boundary\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"qa.txt\"\r\nContent-Transfer-Encoding: base64\r\n\r\nUUEgYXR0YWNobWVudAo=\r\n--qa-boundary--\r\n"
                } else {
                    "Content-Type: text/plain; charset=utf-8\r\n\r\nSynthetic QA message. No private data.\r\n"
                };
                let raw = format!("{headers}{body}").into_bytes();
                let blob = InsertMessageBlobInput::raw(
                    hex::encode(Sha256::digest(&raw)),
                    raw,
                    date.clone(),
                    MessageBlobMetadata {
                        message_id: Some(format!("<qa-{uid}@example.invalid>")),
                        date_header: Some(date.clone()),
                        from_address: Some("qa@example.invalid".into()),
                        to_addresses: Some("recipient@example.invalid".into()),
                        subject: Some(subject),
                        body_text: Some("Synthetic QA message. No private data.".into()),
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
            storage
        }
        Some("verify") => Storage::open_existing(path)?,
        _ => return Err("Usage: qa_fixture create|verify PATH [COUNT]".into()),
    };
    let integrity = storage.verify_integrity()?;
    if !integrity.ok {
        return Err("Synthetic archive failed full integrity verification".into());
    }
    let proof = storage.create_proof_snapshot()?;
    let mut hashes = Vec::new();
    storage.visit_raw_blobs(None, |blob| {
        assert_eq!(blob.sha256, hex::encode(Sha256::digest(&blob.raw_mime)));
        hashes.push(blob.sha256.clone());
        Ok(())
    })?;
    hashes.sort();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "integrity": integrity, "proof": proof, "mime_hashes": hashes
        }))?
    );
    Ok(())
}
