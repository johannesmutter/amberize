//! SQLite storage layer for Amberize.
//!
//! This crate owns schema creation/migrations and all database access.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::Serialize;
use thiserror::Error;

const SCHEMA_VERSION: i64 = 2;

const PRAGMA_JOURNAL_MODE_WAL: &str = "WAL";
const PRAGMA_SYNCHRONOUS_NORMAL: &str = "NORMAL";
const DB_BUSY_TIMEOUT: Duration = Duration::from_secs(2);

/// Classic IMAP provider (password-authenticated).
pub const PROVIDER_KIND_CLASSIC_IMAP: &str = "classic_imap";

/// Google IMAP provider (OAuth-authenticated).
pub const PROVIDER_KIND_GOOGLE_IMAP: &str = "google_imap";

/// Password-based authentication.
pub const AUTH_KIND_PASSWORD: &str = "password";

/// OAuth2-based authentication.
pub const AUTH_KIND_OAUTH2: &str = "oauth2";

/// Google OAuth provider identifier.
pub const OAUTH_PROVIDER_GOOGLE: &str = "google";

const MAILBOX_SELECTION_MODE_AUTO: &str = "auto";

const STORED_ENCODING_RAW: &str = "raw";

const SCHEMA_META_KEY_SCHEMA_VERSION: &str = "schema_version";

pub const EVENT_KIND_SYNC_FINISHED: &str = "sync_finished";
pub const EVENT_KIND_EMAIL_ARCHIVED: &str = "email_archived";

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported stored encoding '{stored_encoding}'")]
    UnsupportedStoredEncoding { stored_encoding: String },

    #[error("unsupported schema version {found} (supported: {supported})")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
}

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Clone)]
pub struct Storage {
    db_path: PathBuf,
}

impl Storage {
    pub fn open_or_create(db_path: impl AsRef<Path>) -> StorageResult<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        create_parent_dir_if_needed(&db_path)?;

        let storage = Self { db_path };
        let mut conn = storage.open_connection()?;
        migrate(&mut conn)?;
        Ok(storage)
    }

    pub fn open_in_memory_for_tests() -> StorageResult<Self> {
        // Note: `:memory:` would create a *separate* database per connection, but this
        // storage abstraction opens new connections per operation. For predictable tests,
        // we use a temp file-backed SQLite DB instead.
        let db_path = test_db_path();
        create_parent_dir_if_needed(&db_path)?;
        let storage = Self { db_path };
        let mut conn = storage.open_connection()?;
        migrate(&mut conn)?;
        Ok(storage)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn schema_version(&self) -> StorageResult<i64> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare("SELECT value FROM schema_meta WHERE key = ?1")?;
        let value: Option<String> = stmt
            .query_row([SCHEMA_META_KEY_SCHEMA_VERSION], |row| row.get(0))
            .optional()?;

        let Some(value) = value else {
            return Ok(0);
        };

        Ok(value.parse::<i64>().unwrap_or(0))
    }

    pub fn create_account(&self, input: &CreateAccountInput) -> StorageResult<i64> {
        let mut conn = self.open_connection()?;
        let now = now_rfc3339();
        let tx = conn.transaction()?;

        tx.execute(
            r#"
      INSERT INTO accounts (
        label,
        email_address,
        provider_kind,
        imap_host,
        imap_port,
        imap_tls,
        imap_username,
        auth_kind,
        secret_ref,
        oauth_provider,
        oauth_user_id,
        oauth_tenant_id,
        oauth_scopes,
        oauth_meta_json,
        mailbox_selection_mode,
        created_at,
        updated_at,
        disabled
      ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
        ?10, NULL, NULL, ?11, NULL,
        ?12, ?13, ?14, 0
      )
      "#,
            params![
                input.label,
                input.email_address,
                input.provider_kind,
                input.imap_host,
                input.imap_port as i64,
                bool_to_int(input.imap_tls),
                input.imap_username,
                input.auth_kind,
                input.secret_ref,
                input.oauth_provider,
                input.oauth_scopes,
                input.mailbox_selection_mode,
                now,
                now
            ],
        )?;

        let account_id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(account_id)
    }

    pub fn set_account_disabled(&self, account_id: i64, disabled: bool) -> StorageResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE accounts SET disabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![bool_to_int(disabled), now_rfc3339(), account_id],
        )?;
        Ok(())
    }

    pub fn list_accounts(&self) -> StorageResult<Vec<AccountRow>> {
        let conn = self.open_connection()?;
        let sql = r#"
      SELECT
        id,
        label,
        email_address,
        provider_kind,
        imap_host,
        imap_port,
        imap_tls,
        imap_username,
        auth_kind,
        secret_ref,
        mailbox_selection_mode,
        created_at,
        updated_at,
        disabled,
        oauth_provider
      FROM accounts
      ORDER BY id ASC
      "#;
        let mut stmt = conn.prepare(sql)?;

        let accounts = stmt
            .query_map([], |row| {
                let port_i64 = row.get::<_, i64>(5)?;
                Ok(AccountRow {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    email_address: row.get(2)?,
                    provider_kind: row.get(3)?,
                    imap_host: row.get(4)?,
                    imap_port: u16::try_from(port_i64.max(0)).unwrap_or(993),
                    imap_tls: int_to_bool(row.get::<_, i64>(6)?),
                    imap_username: row.get(7)?,
                    auth_kind: row.get(8)?,
                    secret_ref: row.get(9)?,
                    mailbox_selection_mode: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                    disabled: int_to_bool(row.get::<_, i64>(13)?),
                    oauth_provider: row.get(14)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(accounts)
    }

    pub fn upsert_mailbox(&self, input: &UpsertMailboxInput) -> StorageResult<i64> {
        let mut conn = self.open_connection()?;
        let now = now_rfc3339();
        let tx = conn.transaction()?;

        // Preserve user-driven enablement if mailbox already exists.
        let sql = r#"
      INSERT INTO mailboxes (
        account_id,
        imap_name,
        delimiter,
        attributes,
        sync_enabled,
        hard_excluded,
        uidvalidity,
        last_seen_uid,
        last_sync_at,
        last_error,
        created_at,
        updated_at
      ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, ?9, ?10
      )
      ON CONFLICT(account_id, imap_name) DO UPDATE SET
        delimiter = excluded.delimiter,
        attributes = excluded.attributes,
        hard_excluded = excluded.hard_excluded,
        uidvalidity = coalesce(excluded.uidvalidity, uidvalidity),
        updated_at = excluded.updated_at
      "#;
        tx.execute(
            sql,
            params![
                input.account_id,
                input.imap_name,
                input.delimiter,
                input.attributes,
                bool_to_int(input.sync_enabled),
                bool_to_int(input.hard_excluded),
                input.uidvalidity.map(|v| v as i64),
                input.last_seen_uid as i64,
                now,
                now
            ],
        )?;

        let mailbox_id = mailbox_id_by_name_tx(&tx, input.account_id, &input.imap_name)?;
        tx.commit()?;
        Ok(mailbox_id)
    }

    /// Return a single mailbox row by id, or `None` if it does not exist.
    pub fn get_mailbox_by_id(&self, mailbox_id: i64) -> StorageResult<Option<MailboxRow>> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, account_id, imap_name, delimiter, attributes, \
             sync_enabled, hard_excluded, uidvalidity, last_seen_uid, \
             last_sync_at, last_error, created_at, updated_at \
             FROM mailboxes WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![mailbox_id], |row| {
            Ok(MailboxRow {
                id: row.get(0)?,
                account_id: row.get(1)?,
                imap_name: row.get(2)?,
                delimiter: row.get(3)?,
                attributes: row.get(4)?,
                sync_enabled: int_to_bool(row.get(5)?),
                hard_excluded: int_to_bool(row.get(6)?),
                uidvalidity: row.get(7)?,
                last_seen_uid: row.get(8)?,
                last_sync_at: row.get(9)?,
                last_error: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn set_mailbox_sync_enabled(
        &self,
        mailbox_id: i64,
        sync_enabled: bool,
    ) -> StorageResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE mailboxes SET sync_enabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![bool_to_int(sync_enabled), now_rfc3339(), mailbox_id],
        )?;
        Ok(())
    }

    pub fn update_mailbox_cursor(
        &self,
        mailbox_id: i64,
        uidvalidity: Option<u32>,
        last_seen_uid: u32,
        last_sync_at: Option<String>,
        last_error: Option<String>,
    ) -> StorageResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        tx.execute(
            r#"
      UPDATE mailboxes
      SET
        uidvalidity = ?1,
        last_seen_uid = ?2,
        last_sync_at = ?3,
        last_error = ?4,
        updated_at = ?5
      WHERE id = ?6
      "#,
            params![
                uidvalidity.map(|v| v as i64),
                last_seen_uid as i64,
                last_sync_at,
                last_error,
                now_rfc3339(),
                mailbox_id
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_mailboxes(&self, account_id: i64) -> StorageResult<Vec<MailboxRow>> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            r#"
      SELECT
        id,
        account_id,
        imap_name,
        delimiter,
        attributes,
        sync_enabled,
        hard_excluded,
        uidvalidity,
        last_seen_uid,
        last_sync_at,
        last_error,
        created_at,
        updated_at
      FROM mailboxes
      WHERE account_id = ?1
      ORDER BY imap_name ASC
      "#,
        )?;

        let mailboxes = stmt
            .query_map([account_id], |row| {
                Ok(MailboxRow {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    imap_name: row.get(2)?,
                    delimiter: row.get(3)?,
                    attributes: row.get(4)?,
                    sync_enabled: int_to_bool(row.get::<_, i64>(5)?),
                    hard_excluded: int_to_bool(row.get::<_, i64>(6)?),
                    uidvalidity: row
                        .get::<_, Option<i64>>(7)?
                        .and_then(|v| u32::try_from(v.max(0)).ok()),
                    last_seen_uid: u32::try_from(row.get::<_, i64>(8)?.max(0)).unwrap_or(0),
                    last_sync_at: row.get(9)?,
                    last_error: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(mailboxes)
    }

    pub fn insert_message_blob_if_absent(
        &self,
        input: &InsertMessageBlobInput,
    ) -> StorageResult<i64> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;

        tx.execute(
            r#"
      INSERT OR IGNORE INTO message_blobs (
        sha256,
        stored_encoding,
        raw_mime,
        raw_mime_size_bytes,
        stored_size_bytes,
        message_id,
        date_header,
        from_address,
        to_addresses,
        cc_addresses,
        subject,
        body_text,
        imported_at
      ) VALUES (
        ?1, ?2, ?3, ?4, ?5,
        ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
      )
      "#,
            params![
                input.sha256,
                input.stored_encoding,
                input.raw_mime,
                input.raw_mime_size_bytes as i64,
                input.stored_size_bytes as i64,
                input.message_id,
                input.date_header,
                input.from_address,
                input.to_addresses,
                input.cc_addresses,
                input.subject,
                input.body_text,
                input.imported_at
            ],
        )?;

        let blob_id = message_blob_id_by_sha256_tx(&tx, &input.sha256)?;
        tx.commit()?;
        Ok(blob_id)
    }

    pub fn upsert_message_location(&self, input: &UpsertMessageLocationInput) -> StorageResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;

        upsert_message_location_tx(&tx, input)?;

        tx.commit()?;
        Ok(())
    }

    /// Atomically insert a message blob (if absent) AND its location in a single transaction.
    /// This prevents "blob without location" orphans that leave the UI empty.
    ///
    /// When a genuinely new blob is inserted, an `email_archived` event is recorded
    /// in the tamper-evident hash chain. The FK from `events.message_blob_id` to
    /// `message_blobs.id` then prevents deletion of the blob without first breaking
    /// the chain.
    pub fn ingest_message(
        &self,
        blob_input: &InsertMessageBlobInput,
        location_input: &IngestMessageLocationInput,
    ) -> StorageResult<i64> {
        let mut conn = self.open_connection()?;
        let mut tx = conn.transaction()?;

        // Insert blob (or find existing by sha256).
        tx.execute(
            r#"
      INSERT OR IGNORE INTO message_blobs (
        sha256, stored_encoding, raw_mime, raw_mime_size_bytes, stored_size_bytes,
        message_id, date_header, from_address, to_addresses, cc_addresses, subject, body_text,
        imported_at
      ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
      )
      "#,
            params![
                blob_input.sha256,
                blob_input.stored_encoding,
                blob_input.raw_mime,
                blob_input.raw_mime_size_bytes as i64,
                blob_input.stored_size_bytes as i64,
                blob_input.message_id,
                blob_input.date_header,
                blob_input.from_address,
                blob_input.to_addresses,
                blob_input.cc_addresses,
                blob_input.subject,
                blob_input.body_text,
                blob_input.imported_at
            ],
        )?;

        let blob_is_new = tx.changes() > 0;

        let blob_id = message_blob_id_by_sha256_tx(&tx, &blob_input.sha256)?;

        // Build full location input with the resolved blob_id.
        let full_location = UpsertMessageLocationInput {
            message_blob_id: blob_id,
            account_id: location_input.account_id,
            mailbox_id: location_input.mailbox_id,
            uidvalidity: location_input.uidvalidity,
            uid: location_input.uid,
            internal_date: location_input.internal_date.clone(),
            flags: location_input.flags.clone(),
            provider_message_id: location_input.provider_message_id.clone(),
            provider_thread_id: location_input.provider_thread_id.clone(),
            provider_labels: location_input.provider_labels.clone(),
            provider_meta_json: location_input.provider_meta_json.clone(),
            first_seen_at: location_input.first_seen_at.clone(),
            last_seen_at: location_input.last_seen_at.clone(),
        };

        upsert_message_location_tx(&tx, &full_location)?;

        // Record the archival in the tamper-evident hash chain so that the FK
        // from events.message_blob_id prevents silent deletion of this blob.
        if blob_is_new {
            insert_event_tx(
                &mut tx,
                &InsertEventInput {
                    occurred_at: now_rfc3339(),
                    kind: EVENT_KIND_EMAIL_ARCHIVED.to_string(),
                    account_id: Some(location_input.account_id),
                    mailbox_id: Some(location_input.mailbox_id),
                    message_blob_id: Some(blob_id),
                    detail: format!(r#"{{"sha256":"{}"}}"#, blob_input.sha256),
                },
            )?;
        }

        tx.commit()?;
        Ok(blob_id)
    }

    pub fn search_message_blobs(
        &self,
        user_query: &str,
        limit: usize,
    ) -> StorageResult<Vec<SearchMessageRow>> {
        let conn = self.open_connection()?;

        match build_fts5_query(user_query) {
            Some(fts_query) => {
                let mut stmt = conn.prepare(
                    r#"
              SELECT
                mb.id,
                mb.subject,
                mb.from_address,
                mb.date_header,
                substr(coalesce(mb.body_text, ''), 1, 200) AS snippet
              FROM messages_fts
              JOIN message_blobs mb ON mb.id = messages_fts.rowid
              WHERE messages_fts MATCH ?1
              ORDER BY bm25(messages_fts)
              LIMIT ?2
              "#,
                )?;

                let results = stmt
                    .query_map(params![fts_query, limit as i64], |row| {
                        Ok(SearchMessageRow {
                            id: row.get(0)?,
                            subject: row.get(1)?,
                            from_address: row.get(2)?,
                            date_header: row.get(3)?,
                            snippet: row.get(4)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(results)
            }
            None => {
                // No valid search tokens — return most recent messages.
                let mut stmt = conn.prepare(
                    r#"
              SELECT
                id,
                subject,
                from_address,
                date_header,
                substr(coalesce(body_text, ''), 1, 200) AS snippet
              FROM message_blobs
              ORDER BY id DESC
              LIMIT ?1
              "#,
                )?;

                let results = stmt
                    .query_map(params![limit as i64], |row| {
                        Ok(SearchMessageRow {
                            id: row.get(0)?,
                            subject: row.get(1)?,
                            from_address: row.get(2)?,
                            date_header: row.get(3)?,
                            snippet: row.get(4)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(results)
            }
        }
    }

    /// List message locations for the UI.
    ///
    /// - `account_id`: optional filter by account
    /// - `mailbox_name`: optional filter by IMAP mailbox name (case-insensitive).
    ///   Pass `None` to list messages from **all** folders.
    /// - `user_query`: FTS search query (empty = list all)
    /// - `limit` / `offset`: pagination
    pub fn list_message_location_rows(
        &self,
        account_id: Option<i64>,
        mailbox_name: Option<&str>,
        user_query: &str,
        limit: usize,
        offset: usize,
    ) -> StorageResult<Vec<MessageLocationListRow>> {
        self.list_message_location_rows_sorted(
            account_id,
            mailbox_name,
            user_query,
            limit,
            offset,
            MessageListSortOrder::NewestFirst,
        )
    }

    /// List message locations for the UI with explicit sort order.
    pub fn list_message_location_rows_sorted(
        &self,
        account_id: Option<i64>,
        mailbox_name: Option<&str>,
        user_query: &str,
        limit: usize,
        offset: usize,
        sort_order: MessageListSortOrder,
    ) -> StorageResult<Vec<MessageLocationListRow>> {
        let conn = self.open_connection()?;
        let order_by_clause = match sort_order {
            MessageListSortOrder::NewestFirst => {
                "coalesce(ml.internal_date, mb.date_header, mb.imported_at) DESC, ml.id DESC"
            }
            MessageListSortOrder::OldestFirst => {
                "coalesce(ml.internal_date, mb.date_header, mb.imported_at) ASC, ml.id ASC"
            }
        };

        match build_fts5_query(user_query) {
            Some(fts_query) => {
                let query = format!(
                    r#"
              SELECT
                ml.id,
                ml.message_blob_id,
                mb.subject,
                mb.from_address,
                coalesce(mb.date_header, ml.internal_date) AS date_header,
                substr(coalesce(mb.body_text, ''), 1, 200) AS snippet,
                a.id,
                a.email_address,
                m.id,
                m.imap_name
              FROM messages_fts
              JOIN message_blobs mb ON mb.id = messages_fts.rowid
              JOIN message_locations ml ON ml.message_blob_id = mb.id
              JOIN mailboxes m ON m.id = ml.mailbox_id
              JOIN accounts a ON a.id = ml.account_id
              WHERE messages_fts MATCH ?1
                AND ml.gone_from_server_at IS NULL
                AND a.disabled = 0
                AND (?2 IS NULL OR m.imap_name = ?2 COLLATE NOCASE)
                AND (?3 IS NULL OR ml.account_id = ?3)
              ORDER BY {order_by_clause}
              LIMIT ?4 OFFSET ?5
              "#,
                );
                let mut stmt = conn.prepare(&query)?;

                let rows = stmt
                    .query_map(
                        params![
                            fts_query,
                            mailbox_name,
                            account_id,
                            limit as i64,
                            offset as i64
                        ],
                        |row| {
                            Ok(MessageLocationListRow {
                                id: row.get(0)?,
                                message_blob_id: row.get(1)?,
                                subject: row.get(2)?,
                                from_address: row.get(3)?,
                                date_header: row.get(4)?,
                                snippet: row.get(5)?,
                                account_id: row.get(6)?,
                                account_email_address: row.get(7)?,
                                mailbox_id: row.get(8)?,
                                mailbox_name: row.get(9)?,
                            })
                        },
                    )?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(rows)
            }
            None => {
                let query = format!(
                    r#"
              SELECT
                ml.id,
                ml.message_blob_id,
                mb.subject,
                mb.from_address,
                coalesce(mb.date_header, ml.internal_date) AS date_header,
                substr(coalesce(mb.body_text, ''), 1, 200) AS snippet,
                a.id,
                a.email_address,
                m.id,
                m.imap_name
              FROM message_locations ml
              JOIN message_blobs mb ON mb.id = ml.message_blob_id
              JOIN mailboxes m ON m.id = ml.mailbox_id
              JOIN accounts a ON a.id = ml.account_id
              WHERE ml.gone_from_server_at IS NULL
                AND a.disabled = 0
                AND (?1 IS NULL OR m.imap_name = ?1 COLLATE NOCASE)
                AND (?2 IS NULL OR ml.account_id = ?2)
              ORDER BY {order_by_clause}
              LIMIT ?3 OFFSET ?4
              "#,
                );
                let mut stmt = conn.prepare(&query)?;

                let rows = stmt
                    .query_map(
                        params![mailbox_name, account_id, limit as i64, offset as i64],
                        |row| {
                            Ok(MessageLocationListRow {
                                id: row.get(0)?,
                                message_blob_id: row.get(1)?,
                                subject: row.get(2)?,
                                from_address: row.get(3)?,
                                date_header: row.get(4)?,
                                snippet: row.get(5)?,
                                account_id: row.get(6)?,
                                account_email_address: row.get(7)?,
                                mailbox_id: row.get(8)?,
                                mailbox_name: row.get(9)?,
                            })
                        },
                    )?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(rows)
            }
        }
    }

    pub fn get_message_blob_raw_mime(&self, message_blob_id: i64) -> StorageResult<MessageBlobRaw> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare("SELECT sha256, stored_encoding, raw_mime FROM message_blobs WHERE id = ?1")?;

        let row = stmt.query_row([message_blob_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;

        let (sha256, stored_encoding, raw_mime) = row;
        if stored_encoding != STORED_ENCODING_RAW {
            return Err(StorageError::UnsupportedStoredEncoding { stored_encoding });
        }

        Ok(MessageBlobRaw {
            id: message_blob_id,
            sha256,
            raw_mime,
        })
    }

    pub fn list_message_blobs_for_export(&self) -> StorageResult<Vec<MessageBlobExportRow>> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare("SELECT id, sha256 FROM message_blobs ORDER BY id ASC")?;

        let rows = stmt
            .query_map([], |row| {
                Ok(MessageBlobExportRow {
                    id: row.get(0)?,
                    sha256: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn list_events_for_export(&self) -> StorageResult<Vec<EventExportRow>> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            r#"
      SELECT
        id,
        occurred_at,
        kind,
        account_id,
        mailbox_id,
        message_blob_id,
        detail,
        prev_hash,
        hash
      FROM events
      ORDER BY id ASC
      "#,
        )?;

        let rows = stmt
            .query_map([], |row| {
                Ok(EventExportRow {
                    id: row.get(0)?,
                    occurred_at: row.get(1)?,
                    kind: row.get(2)?,
                    account_id: row.get(3)?,
                    mailbox_id: row.get(4)?,
                    message_blob_id: row.get(5)?,
                    detail: row.get(6)?,
                    prev_hash: row.get(7)?,
                    hash: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn list_auditor_index_rows(&self) -> StorageResult<Vec<AuditorIndexRow>> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            r#"
      SELECT
        a.id AS account_id,
        a.label AS account_label,
        m.imap_name AS mailbox_name,
        ml.uidvalidity,
        ml.uid,
        ml.internal_date,
        ml.flags,
        mb.id AS message_blob_id,
        mb.sha256,
        mb.message_id,
        mb.date_header,
        mb.from_address,
        mb.to_addresses,
        mb.cc_addresses,
        mb.subject,
        mb.imported_at
      FROM message_locations ml
      JOIN message_blobs mb ON mb.id = ml.message_blob_id
      JOIN mailboxes m ON m.id = ml.mailbox_id
      JOIN accounts a ON a.id = ml.account_id
      ORDER BY a.id ASC, m.imap_name ASC, ml.uid ASC
      "#,
        )?;

        let rows = stmt
            .query_map([], |row| {
                Ok(AuditorIndexRow {
                    account_id: row.get(0)?,
                    account_label: row.get(1)?,
                    mailbox_name: row.get(2)?,
                    uidvalidity: row.get::<_, i64>(3)? as u32,
                    uid: row.get::<_, i64>(4)? as u32,
                    internal_date: row.get(5)?,
                    flags: row.get(6)?,
                    message_blob_id: row.get(7)?,
                    sha256: row.get(8)?,
                    message_id: row.get(9)?,
                    date_header: row.get(10)?,
                    from_address: row.get(11)?,
                    to_addresses: row.get(12)?,
                    cc_addresses: row.get(13)?,
                    subject: row.get(14)?,
                    imported_at: row.get(15)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Record a `sync_finished` event that includes the current archive state
    /// (root hash and blob count) as an integrity checkpoint in the hash chain.
    pub fn create_sync_finished_event(
        &self,
        account_id: i64,
        status: &str,
        messages_imported: u64,
        messages_gone: u64,
    ) -> StorageResult<()> {
        let mut conn = self.open_connection()?;
        let mut tx = conn.transaction()?;

        // Snapshot the archive state inside the transaction for consistency.
        let root_hash = compute_message_blobs_root_hash(&tx)?;
        let blob_count = count_rows(&tx, "message_blobs")?;

        let input = InsertEventInput {
            occurred_at: now_rfc3339(),
            kind: EVENT_KIND_SYNC_FINISHED.to_string(),
            account_id: Some(account_id),
            mailbox_id: None,
            message_blob_id: None,
            detail: format!(
                r#"{{"status":"{}","messages_imported":{},"messages_gone":{},"root_hash":"{}","blob_count":{}}}"#,
                escape_json_string(status),
                messages_imported,
                messages_gone,
                root_hash,
                blob_count,
            ),
        };
        insert_event_tx(&mut tx, &input)?;

        tx.commit()?;
        Ok(())
    }

    pub fn append_event(&self, input: &InsertEventInput) -> StorageResult<i64> {
        let mut conn = self.open_connection()?;
        let mut tx = conn.transaction()?;
        let event_id = insert_event_tx(&mut tx, input)?;
        tx.commit()?;
        Ok(event_id)
    }

    /// Return the most recent events, newest first.
    ///
    /// If `kind_filter` is `Some`, only events with that `kind` are returned.
    /// Results are limited to `limit` rows, starting from offset `offset`.
    pub fn list_recent_events(
        &self,
        kind_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> StorageResult<Vec<EventRow>> {
        let conn = self.open_connection()?;

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match kind_filter {
            Some(kind) => (
                "SELECT id, occurred_at, kind, account_id, mailbox_id, message_blob_id, detail, hash \
                 FROM events WHERE kind = ?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3"
                    .to_string(),
                vec![
                    Box::new(kind.to_string()),
                    Box::new(limit as i64),
                    Box::new(offset as i64),
                ],
            ),
            None => (
                "SELECT id, occurred_at, kind, account_id, mailbox_id, message_blob_id, detail, hash \
                 FROM events ORDER BY id DESC LIMIT ?1 OFFSET ?2"
                    .to_string(),
                vec![Box::new(limit as i64), Box::new(offset as i64)],
            ),
        };

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(EventRow {
                id: row.get(0)?,
                occurred_at: row.get(1)?,
                kind: row.get(2)?,
                account_id: row.get(3)?,
                mailbox_id: row.get(4)?,
                message_blob_id: row.get(5)?,
                detail: row.get(6)?,
                hash: row.get(7)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Return the total number of events, optionally filtered by kind.
    pub fn event_count(&self, kind_filter: Option<&str>) -> StorageResult<u64> {
        let conn = self.open_connection()?;
        let count: u64 = match kind_filter {
            Some(kind) => conn.query_row(
                "SELECT COUNT(*) FROM events WHERE kind = ?1",
                params![kind],
                |row| row.get(0),
            )?,
            None => conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?,
        };
        Ok(count)
    }

    /// Return the `occurred_at` timestamp of the most recent event with the
    /// given `kind`, or `None` if no such event exists.
    pub fn last_event_time_by_kind(&self, kind: &str) -> StorageResult<Option<String>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare("SELECT occurred_at FROM events WHERE kind = ?1 ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query(rusqlite::params![kind])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn create_proof_snapshot(&self) -> StorageResult<ProofSnapshot> {
        let conn = self.open_connection()?;
        let created_at = now_rfc3339();

        let (last_event_id, last_event_hash) = {
            let mut stmt = conn.prepare("SELECT id, hash FROM events ORDER BY id DESC LIMIT 1")?;
            let mut rows = stmt.query([])?;
            match rows.next()? {
                Some(row) => {
                    let id: i64 = row.get(0)?;
                    let hash: String = row.get(1)?;
                    (Some(id), Some(hash))
                }
                None => (None, None),
            }
        };

        let accounts_count = count_rows(&conn, "accounts")?;
        let mailboxes_count = count_rows(&conn, "mailboxes")?;
        let message_blobs_count = count_rows(&conn, "message_blobs")?;
        let message_locations_count = count_rows(&conn, "message_locations")?;
        let events_count = count_rows(&conn, "events")?;

        let message_blobs_root_hash = compute_message_blobs_root_hash(&conn)?;

        Ok(ProofSnapshot {
            created_at,
            last_event_id,
            last_event_hash,
            accounts_count,
            mailboxes_count,
            message_blobs_count,
            message_locations_count,
            events_count,
            message_blobs_root_hash,
        })
    }

    pub fn verify_event_chain(&self) -> StorageResult<EventChainCheckResult> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            r#"
      SELECT
        id,
        occurred_at,
        kind,
        account_id,
        mailbox_id,
        message_blob_id,
        detail,
        prev_hash,
        hash
      FROM events
      ORDER BY id ASC
      "#,
        )?;

        let mut rows = stmt.query([])?;
        let mut previous_hash = "0".repeat(64);
        let mut checked_events = 0_u64;

        while let Some(row) = rows.next()? {
            checked_events += 1;

            let event_id: i64 = row.get(0)?;
            let occurred_at: String = row.get(1)?;
            let kind: String = row.get(2)?;
            let account_id: Option<i64> = row.get(3)?;
            let mailbox_id: Option<i64> = row.get(4)?;
            let message_blob_id: Option<i64> = row.get(5)?;
            let detail: Option<String> = row.get(6)?;
            let prev_hash: String = row.get(7)?;
            let stored_hash: String = row.get(8)?;

            let detail = detail.unwrap_or_else(|| "{}".to_string());

            if prev_hash != previous_hash {
                return Ok(EventChainCheckResult {
                    checked_events,
                    first_mismatch_event_id: Some(event_id),
                });
            }

            let expected_hash = compute_event_hash(
                &prev_hash,
                &InsertEventInput {
                    occurred_at,
                    kind,
                    account_id,
                    mailbox_id,
                    message_blob_id,
                    detail,
                },
            );

            if stored_hash != expected_hash {
                return Ok(EventChainCheckResult {
                    checked_events,
                    first_mismatch_event_id: Some(event_id),
                });
            }

            previous_hash = stored_hash;
        }

        Ok(EventChainCheckResult {
            checked_events,
            first_mismatch_event_id: None,
        })
    }

    pub fn verify_message_blobs_integrity(
        &self,
        max_mismatches: usize,
    ) -> StorageResult<IntegrityCheckResult> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            r#"
      SELECT id, sha256, stored_encoding, raw_mime
      FROM message_blobs
      ORDER BY id ASC
      "#,
        )?;

        let mut rows = stmt.query([])?;
        let mut checked_message_blobs = 0_u64;
        let mut mismatches = Vec::new();

        while let Some(row) = rows.next()? {
            checked_message_blobs += 1;

            let message_blob_id: i64 = row.get(0)?;
            let stored_sha256: String = row.get(1)?;
            let stored_encoding: String = row.get(2)?;
            let raw_mime: Vec<u8> = row.get(3)?;

            if stored_encoding != STORED_ENCODING_RAW {
                mismatches.push(MessageBlobIntegrityMismatch {
                    message_blob_id,
                    stored_sha256,
                    computed_sha256: format!("unsupported stored_encoding={stored_encoding}"),
                });

                if mismatches.len() >= max_mismatches {
                    break;
                }

                continue;
            }

            let computed_sha256 = sha256_hex(&raw_mime);
            if computed_sha256 != stored_sha256 {
                mismatches.push(MessageBlobIntegrityMismatch {
                    message_blob_id,
                    stored_sha256,
                    computed_sha256,
                });

                if mismatches.len() >= max_mismatches {
                    break;
                }
            }
        }

        Ok(IntegrityCheckResult {
            checked_message_blobs,
            mismatches,
        })
    }

    /// Full integrity verification: event chain + root hash checkpoint comparison.
    ///
    /// Returns an `IntegrityStatus` summarising whether the archive is intact.
    pub fn verify_integrity(&self) -> StorageResult<IntegrityStatus> {
        let mut status = IntegrityStatus::default();
        let mut issues: Vec<String> = Vec::new();

        // 1. Verify the event hash chain.
        let chain_result = self.verify_event_chain()?;
        status.chain_checked_events = chain_result.checked_events;
        status.chain_first_mismatch_event_id = chain_result.first_mismatch_event_id;
        status.chain_ok = chain_result.first_mismatch_event_id.is_none();
        if let Some(event_id) = chain_result.first_mismatch_event_id {
            issues.push(format!("Event hash chain broken at event id {event_id}"));
        }

        // 2. Compute the current root hash and blob count.
        let conn = self.open_connection()?;
        status.current_root_hash = compute_message_blobs_root_hash(&conn)?;
        status.current_blob_count = count_rows(&conn, "message_blobs")?;

        // 3. Compare against the most recent sync_finished checkpoint.
        let checkpoint = last_sync_finished_checkpoint(&conn)?;
        if let Some((root_hash, blob_count)) = checkpoint {
            status.checkpoint_root_hash = Some(root_hash.clone());
            status.checkpoint_blob_count = Some(blob_count);

            if root_hash != status.current_root_hash {
                status.root_hash_ok = false;
                issues.push(format!(
                    "Root hash mismatch: checkpoint={root_hash}, current={}",
                    status.current_root_hash
                ));
            } else if blob_count != status.current_blob_count {
                status.root_hash_ok = false;
                issues.push(format!(
                    "Blob count mismatch: checkpoint={blob_count}, current={}",
                    status.current_blob_count
                ));
            } else {
                status.root_hash_ok = true;
            }
        } else {
            // No checkpoint yet — root hash check is vacuously ok.
            status.root_hash_ok = true;
        }

        status.ok = status.chain_ok && status.root_hash_ok;
        status.issues = issues;

        Ok(status)
    }

    /// Quick integrity check: only compare the root hash against the latest
    /// `sync_finished` checkpoint.  Skips the full event chain walk.
    pub fn verify_root_hash_only(&self) -> StorageResult<IntegrityStatus> {
        let mut status = IntegrityStatus::default();
        let mut issues: Vec<String> = Vec::new();

        // Skip chain verification — mark as ok by default.
        status.chain_ok = true;

        let conn = self.open_connection()?;
        status.current_root_hash = compute_message_blobs_root_hash(&conn)?;
        status.current_blob_count = count_rows(&conn, "message_blobs")?;

        let checkpoint = last_sync_finished_checkpoint(&conn)?;
        if let Some((root_hash, blob_count)) = checkpoint {
            status.checkpoint_root_hash = Some(root_hash.clone());
            status.checkpoint_blob_count = Some(blob_count);

            if root_hash != status.current_root_hash {
                status.root_hash_ok = false;
                issues.push(format!(
                    "Root hash mismatch: checkpoint={root_hash}, current={}",
                    status.current_root_hash
                ));
            } else if blob_count != status.current_blob_count {
                status.root_hash_ok = false;
                issues.push(format!(
                    "Blob count mismatch: checkpoint={blob_count}, current={}",
                    status.current_blob_count
                ));
            } else {
                status.root_hash_ok = true;
            }
        } else {
            status.root_hash_ok = true;
        }

        status.ok = status.chain_ok && status.root_hash_ok;
        status.issues = issues;

        Ok(status)
    }

    /// Returns the number of active message locations for a specific account.
    ///
    /// Excludes locations where `gone_from_server_at` is set (deleted from server).
    pub fn count_message_locations_for_account(&self, account_id: i64) -> StorageResult<u64> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "SELECT COUNT(*) FROM message_locations WHERE account_id = ?1 AND gone_from_server_at IS NULL",
        )?;
        let count: i64 = stmt.query_row([account_id], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    /// Returns the oldest and newest archived message dates (if available).
    ///
    /// Uses message location `internal_date` first, falling back to message
    /// metadata `date_header`. This is informational only and does not apply
    /// any retention policy logic.
    pub fn get_archive_date_range(&self) -> StorageResult<ArchiveDateRange> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            r#"SELECT
                 MIN(COALESCE(NULLIF(ml.internal_date, ''), NULLIF(mb.date_header, ''))),
                 MAX(COALESCE(NULLIF(ml.internal_date, ''), NULLIF(mb.date_header, '')))
               FROM message_locations ml
               JOIN message_blobs mb ON mb.id = ml.message_blob_id
               WHERE ml.gone_from_server_at IS NULL"#,
        )?;
        let (oldest_date, newest_date): (Option<String>, Option<String>) =
            stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?)))?;

        Ok(ArchiveDateRange {
            oldest_date,
            newest_date,
        })
    }

    /// Returns a comprehensive diagnostic snapshot of the database state.
    /// This is the "truth extraction" tool — call it once to understand exactly
    /// why the UI shows or hides messages.
    pub fn diagnose_database(&self) -> StorageResult<DatabaseDiagnostic> {
        let conn = self.open_connection()?;

        let accounts_count = count_rows(&conn, "accounts")?;
        let mailboxes_count = count_rows(&conn, "mailboxes")?;
        let message_blobs_count = count_rows(&conn, "message_blobs")?;
        let message_locations_count = count_rows(&conn, "message_locations")?;
        let events_count = count_rows(&conn, "events")?;

        // Accounts with enabled/disabled breakdown.
        let mut stmt = conn.prepare(
            "SELECT id, label, email_address, imap_host, disabled FROM accounts ORDER BY id",
        )?;
        let accounts = stmt
            .query_map([], |row| {
                Ok(DiagnosticAccount {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    email_address: row.get(2)?,
                    imap_host: row.get(3)?,
                    disabled: int_to_bool(row.get::<_, i64>(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Mailbox state.
        let mut stmt = conn.prepare(
            r#"SELECT id, account_id, imap_name, sync_enabled, hard_excluded,
                      uidvalidity, last_seen_uid, last_sync_at,
                      substr(coalesce(last_error, ''), 1, 200)
               FROM mailboxes ORDER BY account_id, imap_name"#,
        )?;
        let mailboxes = stmt
            .query_map([], |row| {
                Ok(DiagnosticMailbox {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    imap_name: row.get(2)?,
                    sync_enabled: int_to_bool(row.get::<_, i64>(3)?),
                    hard_excluded: int_to_bool(row.get::<_, i64>(4)?),
                    uidvalidity: row
                        .get::<_, Option<i64>>(5)?
                        .and_then(|v| u32::try_from(v.max(0)).ok()),
                    last_seen_uid: u32::try_from(row.get::<_, i64>(6)?.max(0)).unwrap_or(0),
                    last_sync_at: row.get(7)?,
                    last_error: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Newest message locations joined with account/mailbox state.
        let mut stmt = conn.prepare(
            r#"SELECT ml.id, ml.message_blob_id, ml.account_id, ml.mailbox_id,
                      ml.uidvalidity, ml.uid, ml.gone_from_server_at,
                      a.disabled, m.imap_name, mb.subject
               FROM message_locations ml
               LEFT JOIN accounts a ON a.id = ml.account_id
               LEFT JOIN mailboxes m ON m.id = ml.mailbox_id
               LEFT JOIN message_blobs mb ON mb.id = ml.message_blob_id
               ORDER BY ml.id DESC LIMIT 20"#,
        )?;
        let recent_locations = stmt
            .query_map([], |row| {
                Ok(DiagnosticLocation {
                    id: row.get(0)?,
                    message_blob_id: row.get(1)?,
                    account_id: row.get(2)?,
                    mailbox_id: row.get(3)?,
                    uidvalidity: row.get::<_, i64>(4)? as u32,
                    uid: row.get::<_, i64>(5)? as u32,
                    gone_from_server_at: row.get(6)?,
                    account_disabled: row.get::<_, Option<i64>>(7).ok().flatten().map(|v| v != 0),
                    mailbox_name: row.get(8)?,
                    subject: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // What the listing query (empty search, no account filter) actually returns.
        let listing_result_count: i64 = conn
            .prepare(
                r#"SELECT COUNT(*)
               FROM message_locations ml
               JOIN message_blobs mb ON mb.id = ml.message_blob_id
               JOIN mailboxes m ON m.id = ml.mailbox_id
               JOIN accounts a ON a.id = ml.account_id
               WHERE ml.gone_from_server_at IS NULL
                 AND a.disabled = 0"#,
            )?
            .query_row([], |row| row.get(0))?;

        let inbox_listing_count: i64 = conn
            .prepare(
                r#"SELECT COUNT(*)
               FROM message_locations ml
               JOIN message_blobs mb ON mb.id = ml.message_blob_id
               JOIN mailboxes m ON m.id = ml.mailbox_id
               JOIN accounts a ON a.id = ml.account_id
               WHERE ml.gone_from_server_at IS NULL
                 AND a.disabled = 0
                 AND m.imap_name = 'INBOX' COLLATE NOCASE"#,
            )?
            .query_row([], |row| row.get(0))?;

        Ok(DatabaseDiagnostic {
            accounts_count,
            mailboxes_count,
            message_blobs_count,
            message_locations_count,
            events_count,
            listing_result_count: listing_result_count as u64,
            inbox_listing_count: inbox_listing_count as u64,
            accounts,
            mailboxes,
            recent_locations,
        })
    }

    /// Resets sync cursor state for all mailboxes of an account, enabling a full resync.
    pub fn reset_mailbox_cursors(&self, account_id: i64) -> StorageResult<u64> {
        let conn = self.open_connection()?;
        let updated = conn.execute(
            r#"UPDATE mailboxes
               SET uidvalidity = NULL, last_seen_uid = 0, last_sync_at = NULL, last_error = NULL,
                   updated_at = ?1
               WHERE account_id = ?2"#,
            params![now_rfc3339(), account_id],
        )?;
        Ok(updated as u64)
    }

    fn open_connection(&self) -> StorageResult<Connection> {
        if self.db_path.as_os_str() == ":memory:" {
            let conn = Connection::open_in_memory()?;
            apply_connection_pragmas(&conn, false)?;
            conn.busy_timeout(DB_BUSY_TIMEOUT)?;
            return Ok(conn);
        }

        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let conn = Connection::open_with_flags(&self.db_path, flags)?;
        apply_connection_pragmas(&conn, true)?;
        conn.busy_timeout(DB_BUSY_TIMEOUT)?;
        Ok(conn)
    }
}

#[derive(Debug, Clone)]
pub struct CreateAccountInput {
    pub label: String,
    pub email_address: String,
    pub provider_kind: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_tls: bool,
    pub imap_username: String,
    pub auth_kind: String,
    pub secret_ref: String,
    pub mailbox_selection_mode: String,
    /// OAuth provider identifier (e.g. `"google"`).  `None` for password auth.
    pub oauth_provider: Option<String>,
    /// OAuth scopes granted (e.g. `"https://mail.google.com/ email"`).
    pub oauth_scopes: Option<String>,
}

impl CreateAccountInput {
    pub fn classic_imap_password(
        label: String,
        email_address: String,
        imap_host: String,
        imap_port: u16,
        imap_tls: bool,
        imap_username: String,
        secret_ref: String,
    ) -> Self {
        Self {
            label,
            email_address,
            provider_kind: PROVIDER_KIND_CLASSIC_IMAP.to_string(),
            imap_host,
            imap_port,
            imap_tls,
            imap_username,
            auth_kind: AUTH_KIND_PASSWORD.to_string(),
            secret_ref,
            mailbox_selection_mode: MAILBOX_SELECTION_MODE_AUTO.to_string(),
            oauth_provider: None,
            oauth_scopes: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccountRow {
    pub id: i64,
    pub label: String,
    pub email_address: String,
    pub provider_kind: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_tls: bool,
    pub imap_username: String,
    pub auth_kind: String,
    pub secret_ref: String,
    pub mailbox_selection_mode: String,
    pub created_at: String,
    pub updated_at: String,
    pub disabled: bool,
    /// OAuth provider (e.g. `"google"`), populated for OAuth accounts.
    pub oauth_provider: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpsertMailboxInput {
    pub account_id: i64,
    pub imap_name: String,
    pub delimiter: Option<String>,
    pub attributes: Option<String>,
    pub sync_enabled: bool,
    pub hard_excluded: bool,
    pub uidvalidity: Option<u32>,
    pub last_seen_uid: u32,
}

#[derive(Debug, Clone)]
pub struct MailboxRow {
    pub id: i64,
    pub account_id: i64,
    pub imap_name: String,
    pub delimiter: Option<String>,
    pub attributes: Option<String>,
    pub sync_enabled: bool,
    pub hard_excluded: bool,
    pub uidvalidity: Option<u32>,
    pub last_seen_uid: u32,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct InsertMessageBlobInput {
    pub sha256: String,
    pub stored_encoding: String,
    pub raw_mime: Vec<u8>,
    pub raw_mime_size_bytes: u64,
    pub stored_size_bytes: u64,
    pub message_id: Option<String>,
    pub date_header: Option<String>,
    pub from_address: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub imported_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct MessageBlobMetadata {
    pub message_id: Option<String>,
    pub date_header: Option<String>,
    pub from_address: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
}

impl InsertMessageBlobInput {
    pub fn raw(
        sha256: String,
        raw_mime: Vec<u8>,
        imported_at: String,
        metadata: MessageBlobMetadata,
    ) -> Self {
        let raw_mime_size_bytes = raw_mime.len() as u64;
        Self {
            sha256,
            stored_encoding: STORED_ENCODING_RAW.to_string(),
            raw_mime,
            raw_mime_size_bytes,
            stored_size_bytes: raw_mime_size_bytes,
            message_id: metadata.message_id,
            date_header: metadata.date_header,
            from_address: metadata.from_address,
            to_addresses: metadata.to_addresses,
            cc_addresses: metadata.cc_addresses,
            subject: metadata.subject,
            body_text: metadata.body_text,
            imported_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpsertMessageLocationInput {
    pub message_blob_id: i64,
    pub account_id: i64,
    pub mailbox_id: i64,
    pub uidvalidity: u32,
    pub uid: u32,
    pub internal_date: Option<String>,
    pub flags: Option<String>,
    pub provider_message_id: Option<String>,
    pub provider_thread_id: Option<String>,
    pub provider_labels: Option<String>,
    pub provider_meta_json: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

/// Like `UpsertMessageLocationInput` but without `message_blob_id` — used with
/// `ingest_message` where the blob ID is resolved inside the transaction.
#[derive(Debug, Clone)]
pub struct IngestMessageLocationInput {
    pub account_id: i64,
    pub mailbox_id: i64,
    pub uidvalidity: u32,
    pub uid: u32,
    pub internal_date: Option<String>,
    pub flags: Option<String>,
    pub provider_message_id: Option<String>,
    pub provider_thread_id: Option<String>,
    pub provider_labels: Option<String>,
    pub provider_meta_json: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

#[derive(Debug, Clone)]
pub struct SearchMessageRow {
    pub id: i64,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub date_header: Option<String>,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct MessageLocationListRow {
    pub id: i64,
    pub message_blob_id: i64,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub date_header: Option<String>,
    pub snippet: String,
    pub account_id: i64,
    pub account_email_address: String,
    pub mailbox_id: i64,
    pub mailbox_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageListSortOrder {
    NewestFirst,
    OldestFirst,
}

#[derive(Debug, Clone)]
pub struct MessageBlobRaw {
    pub id: i64,
    pub sha256: String,
    pub raw_mime: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveDateRange {
    pub oldest_date: Option<String>,
    pub newest_date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProofSnapshot {
    pub created_at: String,
    pub last_event_id: Option<i64>,
    pub last_event_hash: Option<String>,
    pub accounts_count: u64,
    pub mailboxes_count: u64,
    pub message_blobs_count: u64,
    pub message_locations_count: u64,
    pub events_count: u64,
    pub message_blobs_root_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageBlobExportRow {
    pub id: i64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventExportRow {
    pub id: i64,
    pub occurred_at: String,
    pub kind: String,
    pub account_id: Option<i64>,
    pub mailbox_id: Option<i64>,
    pub message_blob_id: Option<i64>,
    pub detail: Option<String>,
    pub prev_hash: String,
    pub hash: String,
}

/// A single event row for the activity-log UI.
#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub occurred_at: String,
    pub kind: String,
    pub account_id: Option<i64>,
    pub mailbox_id: Option<i64>,
    pub message_blob_id: Option<i64>,
    pub detail: Option<String>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditorIndexRow {
    pub account_id: i64,
    pub account_label: String,
    pub mailbox_name: String,
    pub uidvalidity: u32,
    pub uid: u32,
    pub internal_date: Option<String>,
    pub flags: Option<String>,
    pub message_blob_id: i64,
    pub sha256: String,
    pub message_id: Option<String>,
    pub date_header: Option<String>,
    pub from_address: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub subject: Option<String>,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DatabaseDiagnostic {
    pub accounts_count: u64,
    pub mailboxes_count: u64,
    pub message_blobs_count: u64,
    pub message_locations_count: u64,
    pub events_count: u64,
    /// How many message_locations the UI listing query would return (all folders, enabled accounts).
    pub listing_result_count: u64,
    /// How many message_locations match INBOX-only filter.
    pub inbox_listing_count: u64,
    pub accounts: Vec<DiagnosticAccount>,
    pub mailboxes: Vec<DiagnosticMailbox>,
    pub recent_locations: Vec<DiagnosticLocation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticAccount {
    pub id: i64,
    pub label: String,
    pub email_address: String,
    pub imap_host: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticMailbox {
    pub id: i64,
    pub account_id: i64,
    pub imap_name: String,
    pub sync_enabled: bool,
    pub hard_excluded: bool,
    pub uidvalidity: Option<u32>,
    pub last_seen_uid: u32,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticLocation {
    pub id: i64,
    pub message_blob_id: i64,
    pub account_id: i64,
    pub mailbox_id: i64,
    pub uidvalidity: u32,
    pub uid: u32,
    pub gone_from_server_at: Option<String>,
    pub account_disabled: Option<bool>,
    pub mailbox_name: Option<String>,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IntegrityCheckResult {
    pub checked_message_blobs: u64,
    pub mismatches: Vec<MessageBlobIntegrityMismatch>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageBlobIntegrityMismatch {
    pub message_blob_id: i64,
    pub stored_sha256: String,
    pub computed_sha256: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EventChainCheckResult {
    pub checked_events: u64,
    pub first_mismatch_event_id: Option<i64>,
}

/// Result of a full integrity verification covering both the event chain
/// and the message blobs root hash.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IntegrityStatus {
    /// Whether the entire check passed without anomalies.
    pub ok: bool,
    /// Event chain verification result.
    pub chain_ok: bool,
    pub chain_checked_events: u64,
    pub chain_first_mismatch_event_id: Option<i64>,
    /// Root hash comparison against the latest `sync_finished` checkpoint.
    pub root_hash_ok: bool,
    pub current_root_hash: String,
    pub checkpoint_root_hash: Option<String>,
    pub checkpoint_blob_count: Option<u64>,
    pub current_blob_count: u64,
    /// Human-readable summary of any detected issues.
    pub issues: Vec<String>,
}

pub const EVENT_KIND_TAMPERING_DETECTED: &str = "tampering_detected";
pub const EVENT_KIND_INTEGRITY_CHECK: &str = "integrity_check";

#[derive(Debug, Clone)]
pub struct InsertEventInput {
    pub occurred_at: String,
    pub kind: String,
    pub account_id: Option<i64>,
    pub mailbox_id: Option<i64>,
    pub message_blob_id: Option<i64>,
    pub detail: String,
}

fn create_parent_dir_if_needed(db_path: &Path) -> StorageResult<()> {
    let Some(parent) = db_path.parent() else {
        return Ok(());
    };

    if parent.as_os_str().is_empty() {
        return Ok(());
    }

    std::fs::create_dir_all(parent)?;
    Ok(())
}

fn test_db_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "email_archiver_test_{pid}_{nanos}_{counter}.sqlite3"
    ))
}

fn apply_connection_pragmas(conn: &Connection, enable_wal: bool) -> StorageResult<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;

    if enable_wal {
        conn.pragma_update(None, "journal_mode", PRAGMA_JOURNAL_MODE_WAL)?;
    }

    conn.pragma_update(None, "synchronous", PRAGMA_SYNCHRONOUS_NORMAL)?;
    Ok(())
}

fn migrate(conn: &mut Connection) -> StorageResult<()> {
    let tx = conn.transaction()?;
    create_schema_meta_table(&tx)?;
    let existing_version = get_schema_version(&tx)?;

    let Some(existing_version) = existing_version else {
        // Fresh database — create the latest schema in one go.
        create_schema_v1(&tx)?;
        apply_schema_v2(&tx)?;
        set_schema_version(&tx, SCHEMA_VERSION)?;
        tx.commit()?;
        return Ok(());
    };

    if existing_version > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchemaVersion {
            found: existing_version,
            supported: SCHEMA_VERSION,
        });
    }

    // Incremental migrations.
    if existing_version < 2 {
        apply_schema_v2(&tx)?;
        set_schema_version(&tx, 2)?;
    }

    tx.commit()?;
    Ok(())
}

fn create_schema_meta_table(tx: &Transaction<'_>) -> StorageResult<()> {
    tx.execute_batch(
        r#"
    CREATE TABLE IF NOT EXISTS schema_meta (
      key TEXT PRIMARY KEY,
      value TEXT NOT NULL
    );
    "#,
    )?;
    Ok(())
}

fn get_schema_version(tx: &Transaction<'_>) -> StorageResult<Option<i64>> {
    let mut stmt = tx.prepare("SELECT value FROM schema_meta WHERE key = ?1")?;
    let mut rows = stmt.query([SCHEMA_META_KEY_SCHEMA_VERSION])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    let value: String = row.get(0)?;
    let parsed: i64 = value.parse().unwrap_or(0);
    Ok(Some(parsed))
}

fn set_schema_version(tx: &Transaction<'_>, version: i64) -> StorageResult<()> {
    tx.execute(
        r#"
    INSERT INTO schema_meta (key, value) VALUES (?1, ?2)
    ON CONFLICT(key) DO UPDATE SET value = excluded.value
    "#,
        params![SCHEMA_META_KEY_SCHEMA_VERSION, version.to_string()],
    )?;
    Ok(())
}

fn create_schema_v1(tx: &Transaction<'_>) -> StorageResult<()> {
    tx.execute_batch(
    r#"
    CREATE TABLE IF NOT EXISTS accounts (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      label TEXT NOT NULL,
      email_address TEXT NOT NULL,
      provider_kind TEXT NOT NULL DEFAULT 'classic_imap',
      imap_host TEXT NOT NULL,
      imap_port INTEGER NOT NULL,
      imap_tls INTEGER NOT NULL,
      imap_username TEXT NOT NULL,

      auth_kind TEXT NOT NULL,
      secret_ref TEXT NOT NULL,
      oauth_provider TEXT,
      oauth_user_id TEXT,
      oauth_tenant_id TEXT,
      oauth_scopes TEXT,
      oauth_meta_json TEXT,

      mailbox_selection_mode TEXT NOT NULL DEFAULT 'auto',
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      disabled INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS mailboxes (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      account_id INTEGER NOT NULL,
      imap_name TEXT NOT NULL,
      delimiter TEXT,
      attributes TEXT,
      sync_enabled INTEGER NOT NULL DEFAULT 1,
      hard_excluded INTEGER NOT NULL DEFAULT 0,
      uidvalidity INTEGER,
      last_seen_uid INTEGER NOT NULL DEFAULT 0,
      last_sync_at TEXT,
      last_error TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      UNIQUE(account_id, imap_name),
      FOREIGN KEY(account_id) REFERENCES accounts(id)
    );

    CREATE TABLE IF NOT EXISTS message_blobs (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      sha256 TEXT NOT NULL UNIQUE,
      stored_encoding TEXT NOT NULL,
      raw_mime BLOB NOT NULL,
      raw_mime_size_bytes INTEGER NOT NULL,
      stored_size_bytes INTEGER NOT NULL,

      message_id TEXT,
      date_header TEXT,
      from_address TEXT,
      to_addresses TEXT,
      cc_addresses TEXT,
      subject TEXT,
      body_text TEXT,

      imported_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS message_locations (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      message_blob_id INTEGER NOT NULL,
      account_id INTEGER NOT NULL,
      mailbox_id INTEGER NOT NULL,
      uidvalidity INTEGER NOT NULL,
      uid INTEGER NOT NULL,
      internal_date TEXT,
      flags TEXT,
      provider_message_id TEXT,
      provider_thread_id TEXT,
      provider_labels TEXT,
      provider_meta_json TEXT,
      first_seen_at TEXT NOT NULL,
      last_seen_at TEXT NOT NULL,
      gone_from_server_at TEXT,
      UNIQUE(mailbox_id, uidvalidity, uid),
      FOREIGN KEY(message_blob_id) REFERENCES message_blobs(id),
      FOREIGN KEY(account_id) REFERENCES accounts(id),
      FOREIGN KEY(mailbox_id) REFERENCES mailboxes(id)
    );

    CREATE TABLE IF NOT EXISTS events (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      occurred_at TEXT NOT NULL,
      kind TEXT NOT NULL,
      account_id INTEGER,
      mailbox_id INTEGER,
      message_blob_id INTEGER,
      detail TEXT,
      prev_hash TEXT NOT NULL,
      hash TEXT NOT NULL UNIQUE,
      FOREIGN KEY(account_id) REFERENCES accounts(id),
      FOREIGN KEY(mailbox_id) REFERENCES mailboxes(id),
      FOREIGN KEY(message_blob_id) REFERENCES message_blobs(id)
    );

    CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
      subject,
      body_text,
      from_address,
      to_addresses,
      cc_addresses,
      content='message_blobs',
      content_rowid='id'
    );

    CREATE TRIGGER IF NOT EXISTS message_blobs_ai AFTER INSERT ON message_blobs BEGIN
      INSERT INTO messages_fts(rowid, subject, body_text, from_address, to_addresses, cc_addresses)
      VALUES (new.id, new.subject, new.body_text, new.from_address, new.to_addresses, new.cc_addresses);
    END;

    CREATE INDEX IF NOT EXISTS idx_message_locations_provider_message_id
      ON message_locations(account_id, provider_message_id);

    CREATE INDEX IF NOT EXISTS idx_message_locations_provider_thread_id
      ON message_locations(account_id, provider_thread_id);
    "#,
  )?;

    Ok(())
}

/// Schema v2: add BEFORE DELETE triggers on `message_blobs` and `events`
/// to prevent casual deletion of archived data directly in the database.
///
/// These triggers RAISE(ABORT) on any DELETE attempt. They can be bypassed
/// by an attacker who first drops the trigger, but that raises the bar
/// beyond simple `DELETE FROM` statements and leaves forensic evidence in
/// the event chain (the next integrity check will detect the schema change).
fn apply_schema_v2(tx: &Transaction<'_>) -> StorageResult<()> {
    tx.execute_batch(
        r#"
    CREATE TRIGGER IF NOT EXISTS prevent_delete_message_blobs
    BEFORE DELETE ON message_blobs
    BEGIN
      SELECT RAISE(ABORT, 'Deleting archived email blobs is not permitted.');
    END;

    CREATE TRIGGER IF NOT EXISTS prevent_delete_events
    BEFORE DELETE ON events
    BEGIN
      SELECT RAISE(ABORT, 'Deleting events from the audit log is not permitted.');
    END;
    "#,
    )?;
    Ok(())
}

fn upsert_message_location_tx(
    tx: &Transaction<'_>,
    input: &UpsertMessageLocationInput,
) -> StorageResult<()> {
    // Check if this UID already exists with a different blob — log a warning
    // if content changed (rare, but indicates UID reassignment or corruption).
    let existing_blob_id: Option<i64> = tx
        .prepare_cached(
            "SELECT message_blob_id FROM message_locations WHERE mailbox_id = ?1 AND uidvalidity = ?2 AND uid = ?3",
        )?
        .query_row(
            params![input.mailbox_id, input.uidvalidity as i64, input.uid as i64],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(old_blob_id) = existing_blob_id {
        if old_blob_id != input.message_blob_id {
            eprintln!(
                "warning: message_location (mailbox_id={}, uid={}) blob changed {} → {}",
                input.mailbox_id, input.uid, old_blob_id, input.message_blob_id
            );
        }
    }

    tx.execute(
        r#"
    INSERT INTO message_locations (
      message_blob_id, account_id, mailbox_id, uidvalidity, uid,
      internal_date, flags,
      provider_message_id, provider_thread_id, provider_labels, provider_meta_json,
      first_seen_at, last_seen_at, gone_from_server_at
    ) VALUES (
      ?1, ?2, ?3, ?4, ?5, ?6, ?7,
      ?8, ?9, ?10, ?11,
      ?12, ?13, NULL
    )
    ON CONFLICT(mailbox_id, uidvalidity, uid) DO UPDATE SET
      message_blob_id = excluded.message_blob_id,
      internal_date = excluded.internal_date,
      flags = excluded.flags,
      provider_message_id = excluded.provider_message_id,
      provider_thread_id = excluded.provider_thread_id,
      provider_labels = excluded.provider_labels,
      provider_meta_json = excluded.provider_meta_json,
      last_seen_at = excluded.last_seen_at,
      gone_from_server_at = NULL
    "#,
        params![
            input.message_blob_id,
            input.account_id,
            input.mailbox_id,
            input.uidvalidity as i64,
            input.uid as i64,
            input.internal_date,
            input.flags,
            input.provider_message_id,
            input.provider_thread_id,
            input.provider_labels,
            input.provider_meta_json,
            input.first_seen_at,
            input.last_seen_at
        ],
    )?;
    Ok(())
}

fn mailbox_id_by_name_tx(
    tx: &Transaction<'_>,
    account_id: i64,
    imap_name: &str,
) -> StorageResult<i64> {
    let mut stmt =
        tx.prepare("SELECT id FROM mailboxes WHERE account_id = ?1 AND imap_name = ?2")?;
    let id = stmt.query_row(params![account_id, imap_name], |row| row.get(0))?;
    Ok(id)
}

fn message_blob_id_by_sha256_tx(tx: &Transaction<'_>, sha256: &str) -> StorageResult<i64> {
    let mut stmt = tx.prepare("SELECT id FROM message_blobs WHERE sha256 = ?1")?;
    let id = stmt.query_row([sha256], |row| row.get(0))?;
    Ok(id)
}

fn insert_event_tx(tx: &mut Transaction<'_>, input: &InsertEventInput) -> StorageResult<i64> {
    let prev_hash = last_event_hash_tx(tx)?.unwrap_or_else(|| "0".repeat(64));
    let hash = compute_event_hash(&prev_hash, input);

    tx.execute(
        r#"
    INSERT INTO events (
      occurred_at,
      kind,
      account_id,
      mailbox_id,
      message_blob_id,
      detail,
      prev_hash,
      hash
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
    "#,
        params![
            input.occurred_at,
            input.kind,
            input.account_id,
            input.mailbox_id,
            input.message_blob_id,
            input.detail,
            prev_hash,
            hash
        ],
    )?;

    Ok(tx.last_insert_rowid())
}

fn last_event_hash_tx(tx: &Transaction<'_>) -> StorageResult<Option<String>> {
    let mut stmt = tx.prepare("SELECT hash FROM events ORDER BY id DESC LIMIT 1")?;
    let mut rows = stmt.query([])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let hash: String = row.get(0)?;
    Ok(Some(hash))
}

fn compute_event_hash(prev_hash: &str, input: &InsertEventInput) -> String {
    use sha2::Digest;

    let mut hasher = sha2::Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(b"\n");
    hasher.update(input.occurred_at.as_bytes());
    hasher.update(b"\n");
    hasher.update(input.kind.as_bytes());
    hasher.update(b"\n");

    if let Some(account_id) = input.account_id {
        hasher.update(account_id.to_string().as_bytes());
    }
    hasher.update(b"\n");

    if let Some(mailbox_id) = input.mailbox_id {
        hasher.update(mailbox_id.to_string().as_bytes());
    }
    hasher.update(b"\n");

    if let Some(message_blob_id) = input.message_blob_id {
        hasher.update(message_blob_id.to_string().as_bytes());
    }
    hasher.update(b"\n");

    hasher.update(input.detail.as_bytes());

    hex::encode(hasher.finalize())
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}

/// Extract `root_hash` and `blob_count` from the most recent `sync_finished` event.
fn last_sync_finished_checkpoint(conn: &Connection) -> StorageResult<Option<(String, u64)>> {
    let mut stmt =
        conn.prepare("SELECT detail FROM events WHERE kind = ?1 ORDER BY id DESC LIMIT 1")?;
    let mut rows = stmt.query(rusqlite::params![EVENT_KIND_SYNC_FINISHED])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let detail: String = row.get(0)?;

    // Parse the JSON detail to extract root_hash and blob_count.
    // The detail format is: {"status":"...","messages_imported":N,"messages_gone":N,"root_hash":"...","blob_count":N}
    // We use serde_json for robust parsing.
    let parsed: serde_json::Value =
        serde_json::from_str(&detail).unwrap_or(serde_json::Value::Null);

    let root_hash = parsed
        .get("root_hash")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let blob_count = parsed.get("blob_count").and_then(|v| v.as_u64());

    match (root_hash, blob_count) {
        (Some(rh), Some(bc)) => Ok(Some((rh, bc))),
        _ => Ok(None), // Old-format event without checkpoint data.
    }
}

fn count_rows(conn: &Connection, table: &str) -> StorageResult<u64> {
    let query = match table {
        "accounts" => "SELECT COUNT(*) FROM accounts",
        "mailboxes" => "SELECT COUNT(*) FROM mailboxes",
        "message_blobs" => "SELECT COUNT(*) FROM message_blobs",
        "message_locations" => "SELECT COUNT(*) FROM message_locations",
        "events" => "SELECT COUNT(*) FROM events",
        _ => return Ok(0),
    };

    let mut stmt = conn.prepare(query)?;
    let count: i64 = stmt.query_row([], |row| row.get(0))?;
    Ok(count.max(0) as u64)
}

fn compute_message_blobs_root_hash(conn: &Connection) -> StorageResult<String> {
    use sha2::Digest;

    let mut stmt = conn.prepare("SELECT sha256 FROM message_blobs ORDER BY sha256 ASC")?;
    let mut rows = stmt.query([])?;
    let mut hasher = sha2::Sha256::new();

    while let Some(row) = rows.next()? {
        let sha256: String = row.get(0)?;
        hasher.update(sha256.as_bytes());
        hasher.update(b"\n");
    }

    Ok(hex::encode(hasher.finalize()))
}

fn now_rfc3339() -> String {
    // We keep time handling simple and explicit; callers can override via a clock adapter later.
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .expect("RFC3339 formatting should never fail")
}

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn int_to_bool(value: i64) -> bool {
    value != 0
}

fn escape_json_string(input: &str) -> String {
    // Minimal JSON string escaping for small, controlled values.
    // (We keep this here to avoid pulling in more dependencies just for a tiny detail payload.)
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn build_fts5_query(user_query: &str) -> Option<String> {
    let tokens = user_query
        .split_whitespace()
        .filter_map(normalize_fts5_token)
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        return None;
    }

    Some(tokens.join(" AND "))
}

fn normalize_fts5_token(token: &str) -> Option<String> {
    // Allow characters commonly found in email content: alphanumeric,
    // @, ., _, -, + for email addresses and filenames.
    // Also allow unicode letters for international content.
    let normalized = token
        .chars()
        .filter(|c| {
            c.is_alphanumeric() || matches!(c, '@' | '.' | '_' | '-' | '+' | '/' | '\\' | ':')
        })
        .collect::<String>();

    let normalized = normalized.trim_matches(|c: char| !c.is_alphanumeric());
    if normalized.is_empty() {
        return None;
    }

    // Quote the token in double-quotes to escape FTS5 special characters
    // (prevents query injection via AND/OR/NOT operators).
    let escaped = normalized.replace('"', "\"\"");
    Some(format!("\"{escaped}\"*"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn open_in_memory_creates_schema() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        assert!(storage.schema_version().unwrap() >= 1);
    }

    #[test]
    fn can_create_and_list_account() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        assert!(account_id > 0);

        let accounts = storage.list_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].provider_kind, PROVIDER_KIND_CLASSIC_IMAP);
        assert_eq!(accounts[0].auth_kind, AUTH_KIND_PASSWORD);
    }

    #[test]
    fn upsert_mailbox_is_idempotent() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        let inbox_1 = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "INBOX".to_string(),
                delimiter: Some("/".to_string()),
                attributes: Some("\\\\Inbox".to_string()),
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(10),
                last_seen_uid: 0,
            })
            .unwrap();

        let inbox_2 = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "INBOX".to_string(),
                delimiter: Some("/".to_string()),
                attributes: Some("\\\\Inbox".to_string()),
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: None,
                last_seen_uid: 0,
            })
            .unwrap();

        assert_eq!(inbox_1, inbox_2);

        let mailboxes = storage.list_mailboxes(account_id).unwrap();
        assert_eq!(mailboxes.len(), 1);
        assert_eq!(mailboxes[0].uidvalidity, Some(10));
    }

    #[test]
    fn message_blob_dedup_is_by_sha256() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let payload = b"Subject: test\r\n\r\nhello\r\n".to_vec();
        let sha256 = hex::encode(sha2::Sha256::digest(&payload));
        let imported_at = "2026-01-01T00:00:00Z".to_string();

        let id_1 = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha256.clone(),
                payload.clone(),
                imported_at.clone(),
                MessageBlobMetadata {
                    subject: Some("test".to_string()),
                    body_text: Some("hello".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let id_2 = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha256,
                payload,
                imported_at,
                MessageBlobMetadata {
                    subject: Some("test".to_string()),
                    body_text: Some("hello".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        assert_eq!(id_1, id_2);
    }

    #[test]
    fn event_hash_chain_is_deterministic_for_known_inputs() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        storage
            .create_sync_finished_event(account_id, "ok", 5, 0)
            .unwrap();
        storage
            .create_sync_finished_event(account_id, "ok", 3, 0)
            .unwrap();

        // We don't assert the exact hash value (that would be too brittle),
        // but we assert the chain shape: two events, second prev_hash matches first hash.
        let conn = storage.open_connection().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, prev_hash, hash FROM events ORDER BY id ASC")
            .unwrap();
        let rows: Vec<(i64, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].1, rows[0].2);
    }

    #[test]
    fn build_fts5_query_normalizes_tokens() {
        assert_eq!(build_fts5_query(""), None);
        assert_eq!(build_fts5_query("   "), None);

        let query = build_fts5_query("hello world").unwrap();
        assert_eq!(query, "\"hello\"* AND \"world\"*");

        let query = build_fts5_query("re: hello@example.com").unwrap();
        assert_eq!(query, "\"re\"* AND \"hello@example.com\"*");
    }

    #[test]
    fn proof_snapshot_root_hash_is_deterministic() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let imported_at = "2026-01-01T00:00:00Z".to_string();

        let payload_a = b"Subject: a\r\n\r\na\r\n".to_vec();
        let sha_a = sha256_hex(&payload_a);
        let _ = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha_a.clone(),
                payload_a,
                imported_at.clone(),
                MessageBlobMetadata {
                    subject: Some("a".to_string()),
                    body_text: Some("a".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let payload_b = b"Subject: b\r\n\r\nb\r\n".to_vec();
        let sha_b = sha256_hex(&payload_b);
        let _ = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha_b.clone(),
                payload_b,
                imported_at,
                MessageBlobMetadata {
                    subject: Some("b".to_string()),
                    body_text: Some("b".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let snapshot = storage.create_proof_snapshot().unwrap();

        let mut shas = [sha_a, sha_b];
        shas.sort();
        let concatenated = format!("{}\n{}\n", shas[0], shas[1]);
        let expected = sha256_hex(concatenated.as_bytes());
        assert_eq!(snapshot.message_blobs_root_hash, expected);
    }

    #[test]
    fn verify_event_chain_returns_ok_for_valid_chain() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        storage
            .create_sync_finished_event(account_id, "ok", 5, 0)
            .unwrap();
        storage
            .create_sync_finished_event(account_id, "ok", 3, 0)
            .unwrap();

        let result = storage.verify_event_chain().unwrap();
        assert_eq!(result.first_mismatch_event_id, None);
        assert!(result.checked_events >= 2);
    }

    #[test]
    fn proof_snapshot_records_last_event_when_present() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        storage
            .create_sync_finished_event(account_id, "ok", 1, 0)
            .unwrap();
        let snapshot = storage.create_proof_snapshot().unwrap();
        assert_eq!(snapshot.last_event_id, Some(1));
        assert!(snapshot.last_event_hash.as_deref().unwrap_or("").len() >= 64);
    }

    #[test]
    fn verify_event_chain_detects_prev_hash_mismatch() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        storage
            .create_sync_finished_event(account_id, "ok", 5, 0)
            .unwrap();
        storage
            .create_sync_finished_event(account_id, "ok", 3, 0)
            .unwrap();

        let conn = storage.open_connection().unwrap();
        conn.execute(
            "UPDATE events SET prev_hash = ?1 WHERE id = 2",
            params!["1".repeat(64)],
        )
        .unwrap();

        let result = storage.verify_event_chain().unwrap();
        assert_eq!(result.first_mismatch_event_id, Some(2));
        assert!(result.checked_events >= 2);
    }

    #[test]
    fn verify_event_chain_detects_hash_mismatch() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        storage
            .create_sync_finished_event(account_id, "ok", 1, 0)
            .unwrap();

        let conn = storage.open_connection().unwrap();
        conn.execute(
            "UPDATE events SET hash = ?1 WHERE id = 1",
            params!["0".repeat(64)],
        )
        .unwrap();

        let result = storage.verify_event_chain().unwrap();
        assert_eq!(result.first_mismatch_event_id, Some(1));
        assert!(result.checked_events >= 1);
    }

    #[test]
    fn verify_message_blobs_integrity_detects_tamper() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let payload = b"Subject: test\r\n\r\nhello\r\n".to_vec();
        let sha256 = sha256_hex(&payload);
        let imported_at = "2026-01-01T00:00:00Z".to_string();

        let blob_id = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha256,
                payload,
                imported_at,
                MessageBlobMetadata {
                    subject: Some("test".to_string()),
                    body_text: Some("hello".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let conn = storage.open_connection().unwrap();
        conn.execute(
            "UPDATE message_blobs SET raw_mime = ?1 WHERE id = ?2",
            params![b"tampered".to_vec(), blob_id],
        )
        .unwrap();

        let report = storage.verify_message_blobs_integrity(10).unwrap();
        assert_eq!(report.checked_message_blobs, 1);
        assert_eq!(report.mismatches.len(), 1);
        assert_eq!(report.mismatches[0].message_blob_id, blob_id);
    }

    #[test]
    fn verify_message_blobs_integrity_reports_unsupported_encoding_and_can_stop_early() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let payload_a = b"Subject: a\r\n\r\na\r\n".to_vec();
        let sha_a = sha256_hex(&payload_a);
        let imported_at = "2026-01-01T00:00:00Z".to_string();

        let _ = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput {
                sha256: sha_a.clone(),
                stored_encoding: "gzip".to_string(),
                raw_mime: payload_a,
                raw_mime_size_bytes: 10,
                stored_size_bytes: 10,
                message_id: None,
                date_header: None,
                from_address: None,
                to_addresses: None,
                cc_addresses: None,
                subject: None,
                body_text: None,
                imported_at: imported_at.clone(),
            })
            .unwrap();

        let payload_b = b"Subject: b\r\n\r\nb\r\n".to_vec();
        let sha_b = sha256_hex(&payload_b);
        let _ = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha_b,
                payload_b,
                imported_at,
                MessageBlobMetadata {
                    subject: Some("b".to_string()),
                    body_text: Some("b".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let report = storage.verify_message_blobs_integrity(10).unwrap();
        assert_eq!(report.mismatches.len(), 1);
        assert!(report.mismatches[0]
            .computed_sha256
            .starts_with("unsupported stored_encoding="));

        let report = storage.verify_message_blobs_integrity(1).unwrap();
        assert_eq!(report.mismatches.len(), 1);
        assert_eq!(report.checked_message_blobs, 1);
    }

    #[test]
    fn verify_message_blobs_integrity_can_stop_after_sha_mismatch() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let payload_a = b"Subject: a\r\n\r\na\r\n".to_vec();
        let sha_a = sha256_hex(&payload_a);
        let imported_at = "2026-01-01T00:00:00Z".to_string();
        let blob_id_a = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha_a,
                payload_a,
                imported_at.clone(),
                MessageBlobMetadata {
                    subject: Some("a".to_string()),
                    body_text: Some("a".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let conn = storage.open_connection().unwrap();
        conn.execute(
            "UPDATE message_blobs SET raw_mime = ?1 WHERE id = ?2",
            params![b"tampered".to_vec(), blob_id_a],
        )
        .unwrap();

        let payload_b = b"Subject: b\r\n\r\nb\r\n".to_vec();
        let sha_b = sha256_hex(&payload_b);
        let _ = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha_b,
                payload_b,
                imported_at,
                MessageBlobMetadata {
                    subject: Some("b".to_string()),
                    body_text: Some("b".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let report = storage.verify_message_blobs_integrity(1).unwrap();
        assert_eq!(report.mismatches.len(), 1);
        assert_eq!(report.checked_message_blobs, 1);
    }

    #[test]
    fn export_queries_return_expected_rows() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        let mailbox_id = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "INBOX".to_string(),
                delimiter: Some("/".to_string()),
                attributes: Some("\\\\Inbox".to_string()),
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(10),
                last_seen_uid: 0,
            })
            .unwrap();

        let payload = b"Subject: test\r\n\r\nhello\r\n".to_vec();
        let sha256 = sha256_hex(&payload);
        let imported_at = "2026-01-01T00:00:00Z".to_string();

        let blob_id = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha256.clone(),
                payload,
                imported_at.clone(),
                MessageBlobMetadata {
                    message_id: Some("<id@example.com>".to_string()),
                    date_header: Some("2026-01-01T00:00:00Z".to_string()),
                    from_address: Some("sender@example.com".to_string()),
                    to_addresses: Some("to@example.com".to_string()),
                    subject: Some("test".to_string()),
                    body_text: Some("hello".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        storage
            .upsert_message_location(&UpsertMessageLocationInput {
                message_blob_id: blob_id,
                account_id,
                mailbox_id,
                uidvalidity: 10,
                uid: 1,
                internal_date: Some("2026-01-01T00:00:00Z".to_string()),
                flags: Some("\\\\Seen".to_string()),
                provider_message_id: None,
                provider_thread_id: None,
                provider_labels: None,
                provider_meta_json: None,
                first_seen_at: imported_at.clone(),
                last_seen_at: imported_at,
            })
            .unwrap();

        storage
            .create_sync_finished_event(account_id, "ok", 1, 0)
            .unwrap();

        let blobs = storage.list_message_blobs_for_export().unwrap();
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].id, blob_id);
        assert_eq!(blobs[0].sha256, sha256);

        let events = storage.list_events_for_export().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EVENT_KIND_SYNC_FINISHED);

        let index_rows = storage.list_auditor_index_rows().unwrap();
        assert_eq!(index_rows.len(), 1);
        assert_eq!(index_rows[0].account_id, account_id);
        assert_eq!(index_rows[0].mailbox_name, "INBOX");
        assert_eq!(index_rows[0].message_blob_id, blob_id);
    }

    #[test]
    fn open_or_create_creates_parent_dir_and_reports_db_path() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        let db_path = std::env::temp_dir()
            .join(format!("email_archiver_open_or_create_{nanos}"))
            .join("nested")
            .join("archive.sqlite3");

        let storage = Storage::open_or_create(&db_path).unwrap();
        assert_eq!(storage.db_path(), db_path.as_path());
        assert_eq!(storage.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn schema_version_returns_0_when_missing_or_invalid() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let conn = storage.open_connection().unwrap();

        conn.execute(
            "DELETE FROM schema_meta WHERE key = ?1",
            params![SCHEMA_META_KEY_SCHEMA_VERSION],
        )
        .unwrap();
        assert_eq!(storage.schema_version().unwrap(), 0);

        conn.execute(
            "INSERT INTO schema_meta (key, value) VALUES (?1, ?2)",
            params![SCHEMA_META_KEY_SCHEMA_VERSION, "not-a-number"],
        )
        .unwrap();
        assert_eq!(storage.schema_version().unwrap(), 0);
    }

    #[test]
    fn can_disable_account() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        storage.set_account_disabled(account_id, true).unwrap();
        let accounts = storage.list_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert!(accounts[0].disabled);
    }

    #[test]
    fn can_update_mailbox_sync_and_cursor() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        let mailbox_id = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "INBOX".to_string(),
                delimiter: Some("/".to_string()),
                attributes: None,
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(10),
                last_seen_uid: 0,
            })
            .unwrap();

        storage.set_mailbox_sync_enabled(mailbox_id, false).unwrap();
        storage
            .update_mailbox_cursor(
                mailbox_id,
                Some(20),
                42,
                Some("2026-01-01T00:00:00Z".to_string()),
                Some("ok".to_string()),
            )
            .unwrap();

        let mailboxes = storage.list_mailboxes(account_id).unwrap();
        assert_eq!(mailboxes.len(), 1);
        assert!(!mailboxes[0].sync_enabled);
        assert_eq!(mailboxes[0].uidvalidity, Some(20));
        assert_eq!(mailboxes[0].last_seen_uid, 42);
        assert_eq!(
            mailboxes[0].last_sync_at.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(mailboxes[0].last_error.as_deref(), Some("ok"));
    }

    #[test]
    fn can_search_message_blobs() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        assert!(storage.search_message_blobs("   ", 10).unwrap().is_empty());

        let payload = b"Subject: hello\r\n\r\nhello world\r\n".to_vec();
        let sha256 = sha256_hex(&payload);
        let imported_at = "2026-01-01T00:00:00Z".to_string();
        let blob_id = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha256.clone(),
                payload.clone(),
                imported_at,
                MessageBlobMetadata {
                    from_address: Some("sender@example.com".to_string()),
                    subject: Some("hello".to_string()),
                    body_text: Some("hello world".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let results = storage.search_message_blobs("hello", 10).unwrap();
        assert!(results.iter().any(|row| row.id == blob_id));

        // Wildcard and empty queries fall back to listing all messages.
        let results = storage.search_message_blobs("*", 10).unwrap();
        assert!(results.iter().any(|row| row.id == blob_id));

        let results = storage.search_message_blobs("", 10).unwrap();
        assert!(results.iter().any(|row| row.id == blob_id));

        let results = storage.search_message_blobs("   ", 10).unwrap();
        assert!(results.iter().any(|row| row.id == blob_id));
    }

    #[test]
    fn get_message_blob_raw_mime_rejects_unsupported_encoding() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        let payload = b"Subject: test\r\n\r\nhello\r\n".to_vec();
        let sha256 = sha256_hex(&payload);
        let imported_at = "2026-01-01T00:00:00Z".to_string();
        let blob_id = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha256.clone(),
                payload.clone(),
                imported_at,
                MessageBlobMetadata {
                    subject: Some("test".to_string()),
                    body_text: Some("hello".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let raw = storage.get_message_blob_raw_mime(blob_id).unwrap();
        assert_eq!(raw.id, blob_id);
        assert_eq!(raw.sha256, sha256);
        assert_eq!(raw.raw_mime, payload);

        let conn = storage.open_connection().unwrap();
        conn.execute(
            "UPDATE message_blobs SET stored_encoding = ?1 WHERE id = ?2",
            params!["gzip", blob_id],
        )
        .unwrap();

        let err = storage.get_message_blob_raw_mime(blob_id).unwrap_err();
        assert!(matches!(
            err,
            StorageError::UnsupportedStoredEncoding { .. }
        ));
    }

    #[test]
    fn append_event_supports_mailbox_and_message_blob_ids() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Primary".to_string(),
                "user@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "user@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        let mailbox_id = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "INBOX".to_string(),
                delimiter: None,
                attributes: None,
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(10),
                last_seen_uid: 0,
            })
            .unwrap();

        let payload = b"Subject: event\r\n\r\nevent\r\n".to_vec();
        let sha256 = sha256_hex(&payload);
        let imported_at = "2026-01-01T00:00:00Z".to_string();
        let blob_id = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha256,
                payload,
                imported_at,
                MessageBlobMetadata {
                    subject: Some("event".to_string()),
                    body_text: Some("event".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let event_id = storage
            .append_event(&InsertEventInput {
                occurred_at: "2026-01-01T00:00:00Z".to_string(),
                kind: "custom".to_string(),
                account_id: Some(account_id),
                mailbox_id: Some(mailbox_id),
                message_blob_id: Some(blob_id),
                detail: r#"{"v":1}"#.to_string(),
            })
            .unwrap();

        assert!(event_id > 0);
        let events = storage.list_events_for_export().unwrap();
        assert!(events.iter().any(|row| row.id == event_id));

        let conn = storage.open_connection().unwrap();
        assert_eq!(count_rows(&conn, "unknown").unwrap(), 0);
    }

    #[test]
    fn open_or_create_rejects_unsupported_schema_versions() {
        let db_path = test_db_path();
        create_parent_dir_if_needed(&db_path).unwrap();

        let mut conn = Connection::open(&db_path).unwrap();
        apply_connection_pragmas(&conn, true).unwrap();
        conn.busy_timeout(DB_BUSY_TIMEOUT).unwrap();

        let tx = conn.transaction().unwrap();
        create_schema_meta_table(&tx).unwrap();
        set_schema_version(&tx, 999).unwrap();
        tx.commit().unwrap();

        let err = Storage::open_or_create(&db_path).unwrap_err();
        assert!(matches!(err, StorageError::UnsupportedSchemaVersion { .. }));
    }

    #[test]
    fn open_or_create_supports_memory_mode_for_migrations() {
        let storage = Storage::open_or_create(":memory:").unwrap();
        assert_eq!(storage.db_path().as_os_str(), ":memory:");

        // cover create_parent_dir_if_needed edge cases
        create_parent_dir_if_needed(Path::new("/")).unwrap();
        create_parent_dir_if_needed(Path::new("relative.sqlite3")).unwrap();
    }

    #[test]
    fn open_or_create_on_existing_db_commits_when_version_matches() {
        let db_path = test_db_path();
        let _ = Storage::open_or_create(&db_path).unwrap();
        let reopened = Storage::open_or_create(&db_path).unwrap();
        assert_eq!(reopened.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn normalize_fts5_token_returns_none_when_only_symbols() {
        assert_eq!(normalize_fts5_token("++"), None);
        assert_eq!(normalize_fts5_token("---"), None);
    }

    // ── Testing Lab: comprehensive integration tests ──────────────────────

    /// Helper: creates a storage with one account, one mailbox (INBOX), and returns (storage, account_id, mailbox_id).
    fn setup_test_account_with_inbox() -> (Storage, i64, i64) {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Test".to_string(),
                "test@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "test@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        let mailbox_id = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "INBOX".to_string(),
                delimiter: Some("/".to_string()),
                attributes: None,
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(100),
                last_seen_uid: 0,
            })
            .unwrap();

        (storage, account_id, mailbox_id)
    }

    /// Helper: creates a simple test message payload and its sha256.
    fn test_message_payload(subject: &str, body: &str) -> (Vec<u8>, String) {
        let payload = format!("Subject: {subject}\r\n\r\n{body}\r\n").into_bytes();
        let sha = sha256_hex(&payload);
        (payload, sha)
    }

    #[test]
    fn ingest_message_creates_blob_and_location_atomically() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        let (payload, sha) = test_message_payload("atomic test", "hello");
        let imported_at = "2026-01-01T00:00:00Z".to_string();
        let now = imported_at.clone();

        let blob_id = storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    sha.clone(),
                    payload,
                    imported_at,
                    MessageBlobMetadata {
                        subject: Some("atomic test".to_string()),
                        body_text: Some("hello".to_string()),
                        from_address: Some("sender@example.com".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id,
                    uidvalidity: 100,
                    uid: 1,
                    internal_date: Some("2026-01-01T00:00:00Z".to_string()),
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .unwrap();

        assert!(blob_id > 0);

        // Verify both blob AND location exist.
        let conn = storage.open_connection().unwrap();
        let blob_count: i64 = conn
            .prepare("SELECT COUNT(*) FROM message_blobs WHERE id = ?1")
            .unwrap()
            .query_row([blob_id], |r| r.get(0))
            .unwrap();
        assert_eq!(blob_count, 1);

        let location_count: i64 = conn
            .prepare("SELECT COUNT(*) FROM message_locations WHERE message_blob_id = ?1")
            .unwrap()
            .query_row([blob_id], |r| r.get(0))
            .unwrap();
        assert_eq!(location_count, 1);
    }

    #[test]
    fn ingest_message_dedup_blob_but_creates_second_location() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        // Create a second mailbox ("Sent").
        let sent_id = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "Sent".to_string(),
                delimiter: Some("/".to_string()),
                attributes: None,
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(200),
                last_seen_uid: 0,
            })
            .unwrap();

        let (payload, sha) = test_message_payload("dedup test", "same body");
        let imported_at = "2026-01-01T00:00:00Z".to_string();
        let now = imported_at.clone();

        // Ingest once into INBOX.
        let blob_id_1 = storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    sha.clone(),
                    payload.clone(),
                    imported_at.clone(),
                    MessageBlobMetadata {
                        subject: Some("dedup test".to_string()),
                        body_text: Some("same body".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id,
                    uidvalidity: 100,
                    uid: 1,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now.clone(),
                },
            )
            .unwrap();

        // Ingest same message into Sent (same sha256, different mailbox).
        let blob_id_2 = storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    sha,
                    payload,
                    imported_at,
                    MessageBlobMetadata {
                        subject: Some("dedup test".to_string()),
                        body_text: Some("same body".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id: sent_id,
                    uidvalidity: 200,
                    uid: 1,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .unwrap();

        // Same blob_id (dedup).
        assert_eq!(blob_id_1, blob_id_2);

        // But two distinct message_locations.
        let conn = storage.open_connection().unwrap();
        let location_count: i64 = conn
            .prepare("SELECT COUNT(*) FROM message_locations WHERE message_blob_id = ?1")
            .unwrap()
            .query_row([blob_id_1], |r| r.get(0))
            .unwrap();
        assert_eq!(location_count, 2);
    }

    #[test]
    fn list_message_location_rows_shows_all_folders_when_mailbox_name_is_none() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        let sent_id = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "Sent".to_string(),
                delimiter: Some("/".to_string()),
                attributes: None,
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(200),
                last_seen_uid: 0,
            })
            .unwrap();

        let now = "2026-01-01T00:00:00Z".to_string();

        // Ingest into INBOX.
        let (p1, s1) = test_message_payload("inbox msg", "inbox body");
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    s1,
                    p1,
                    now.clone(),
                    MessageBlobMetadata {
                        subject: Some("inbox msg".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id,
                    uidvalidity: 100,
                    uid: 1,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now.clone(),
                },
            )
            .unwrap();

        // Ingest into Sent.
        let (p2, s2) = test_message_payload("sent msg", "sent body");
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    s2,
                    p2,
                    now.clone(),
                    MessageBlobMetadata {
                        subject: Some("sent msg".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id: sent_id,
                    uidvalidity: 200,
                    uid: 1,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .unwrap();

        // mailbox_name = None → all folders.
        let all = storage
            .list_message_location_rows(None, None, "", 100, 0)
            .unwrap();
        assert_eq!(all.len(), 2, "should see messages from both INBOX and Sent");

        // mailbox_name = Some("INBOX") → only INBOX.
        let inbox_only = storage
            .list_message_location_rows(None, Some("INBOX"), "", 100, 0)
            .unwrap();
        assert_eq!(inbox_only.len(), 1, "should see only INBOX message");
        assert_eq!(inbox_only[0].mailbox_name, "INBOX");

        // mailbox_name = Some("Sent") → only Sent.
        let sent_only = storage
            .list_message_location_rows(None, Some("Sent"), "", 100, 0)
            .unwrap();
        assert_eq!(sent_only.len(), 1, "should see only Sent message");
        assert_eq!(sent_only[0].mailbox_name, "Sent");
    }

    #[test]
    fn list_message_location_rows_hides_disabled_accounts() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();
        let now = "2026-01-01T00:00:00Z".to_string();

        let (p, s) = test_message_payload("disabled test", "body");
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    s,
                    p,
                    now.clone(),
                    MessageBlobMetadata {
                        subject: Some("disabled test".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id,
                    uidvalidity: 100,
                    uid: 1,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .unwrap();

        // Visible before disabling.
        let visible = storage
            .list_message_location_rows(None, None, "", 100, 0)
            .unwrap();
        assert_eq!(visible.len(), 1);

        // Disable the account.
        storage.set_account_disabled(account_id, true).unwrap();

        // Hidden after disabling.
        let hidden = storage
            .list_message_location_rows(None, None, "", 100, 0)
            .unwrap();
        assert_eq!(
            hidden.len(),
            0,
            "disabled account messages should be hidden"
        );
    }

    #[test]
    fn diagnose_database_returns_correct_counts() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();
        let now = "2026-01-01T00:00:00Z".to_string();

        let (p, s) = test_message_payload("diag test", "body");
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    s,
                    p,
                    now.clone(),
                    MessageBlobMetadata {
                        subject: Some("diag test".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id,
                    uidvalidity: 100,
                    uid: 1,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .unwrap();

        let diag = storage.diagnose_database().unwrap();
        assert_eq!(diag.accounts_count, 1);
        assert_eq!(diag.mailboxes_count, 1);
        assert_eq!(diag.message_blobs_count, 1);
        assert_eq!(diag.message_locations_count, 1);
        assert_eq!(diag.listing_result_count, 1);
        assert_eq!(diag.inbox_listing_count, 1);

        assert_eq!(diag.accounts.len(), 1);
        assert!(!diag.accounts[0].disabled);

        assert_eq!(diag.recent_locations.len(), 1);
        assert_eq!(
            diag.recent_locations[0].mailbox_name.as_deref(),
            Some("INBOX")
        );
    }

    #[test]
    fn reset_mailbox_cursors_clears_sync_state() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        // Simulate a sync having run.
        storage
            .update_mailbox_cursor(
                mailbox_id,
                Some(100),
                42,
                Some("2026-01-01T00:00:00Z".to_string()),
                None,
            )
            .unwrap();

        let mailboxes = storage.list_mailboxes(account_id).unwrap();
        assert_eq!(mailboxes[0].last_seen_uid, 42);
        assert_eq!(mailboxes[0].uidvalidity, Some(100));

        // Reset.
        let updated = storage.reset_mailbox_cursors(account_id).unwrap();
        assert_eq!(updated, 1);

        // Verify reset.
        let mailboxes = storage.list_mailboxes(account_id).unwrap();
        assert_eq!(mailboxes[0].last_seen_uid, 0);
        assert_eq!(mailboxes[0].uidvalidity, None);
        assert_eq!(mailboxes[0].last_sync_at, None);
        assert_eq!(mailboxes[0].last_error, None);
    }

    #[test]
    fn diagnose_reveals_blob_without_location_problem() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let account_id = storage
            .create_account(&CreateAccountInput::classic_imap_password(
                "Test".to_string(),
                "test@example.com".to_string(),
                "imap.example.com".to_string(),
                993,
                true,
                "test@example.com".to_string(),
                "account/test".to_string(),
            ))
            .unwrap();

        let _mailbox_id = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "INBOX".to_string(),
                delimiter: None,
                attributes: None,
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(100),
                last_seen_uid: 0,
            })
            .unwrap();

        // Insert a blob WITHOUT a corresponding location (simulates the old non-atomic bug).
        let (payload, sha) = test_message_payload("orphan blob", "orphan body");
        let _blob_id = storage
            .insert_message_blob_if_absent(&InsertMessageBlobInput::raw(
                sha,
                payload,
                "2026-01-01T00:00:00Z".to_string(),
                MessageBlobMetadata {
                    subject: Some("orphan blob".to_string()),
                    ..Default::default()
                },
            ))
            .unwrap();

        let diag = storage.diagnose_database().unwrap();
        assert_eq!(diag.message_blobs_count, 1, "blob exists");
        assert_eq!(diag.message_locations_count, 0, "no location");
        assert_eq!(diag.listing_result_count, 0, "nothing visible in UI");
        assert_eq!(diag.recent_locations.len(), 0, "no locations to show");
    }

    // -----------------------------------------------------------------------
    // Integrity hardening tests
    // -----------------------------------------------------------------------

    /// Helper to ingest a test message via `ingest_message` (new API with events).
    fn ingest_test_message(
        storage: &Storage,
        account_id: i64,
        mailbox_id: i64,
        subject: &str,
        uid: u32,
    ) -> i64 {
        let (payload, sha) = test_message_payload(subject, &format!("body for {subject}"));
        let now = "2026-01-01T00:00:00Z".to_string();
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    sha,
                    payload,
                    now.clone(),
                    MessageBlobMetadata {
                        subject: Some(subject.to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id,
                    uidvalidity: 100,
                    uid,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .unwrap()
    }

    #[test]
    fn ingest_message_creates_email_archived_event() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();
        let blob_id = ingest_test_message(&storage, account_id, mailbox_id, "test", 1);

        // There should be exactly one email_archived event referencing this blob.
        let conn = storage.open_connection().unwrap();
        let count: i64 = conn
            .prepare("SELECT COUNT(*) FROM events WHERE kind = ?1 AND message_blob_id = ?2")
            .unwrap()
            .query_row(rusqlite::params![EVENT_KIND_EMAIL_ARCHIVED, blob_id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn duplicate_ingest_skips_duplicate_event() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        // Create a second mailbox.
        let sent_id = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: "Sent".to_string(),
                delimiter: Some("/".to_string()),
                attributes: None,
                sync_enabled: true,
                hard_excluded: false,
                uidvalidity: Some(200),
                last_seen_uid: 0,
            })
            .unwrap();

        let (payload, sha) = test_message_payload("dedup", "same body");
        let now = "2026-01-01T00:00:00Z".to_string();

        // Ingest once → should create event.
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    sha.clone(),
                    payload.clone(),
                    now.clone(),
                    MessageBlobMetadata {
                        subject: Some("dedup".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id,
                    uidvalidity: 100,
                    uid: 1,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now.clone(),
                },
            )
            .unwrap();

        // Ingest again (same sha256, different location) → should NOT create event.
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(
                    sha.clone(),
                    payload,
                    now.clone(),
                    MessageBlobMetadata {
                        subject: Some("dedup".to_string()),
                        ..Default::default()
                    },
                ),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id: sent_id,
                    uidvalidity: 200,
                    uid: 1,
                    internal_date: None,
                    flags: None,
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .unwrap();

        let conn = storage.open_connection().unwrap();
        let count: i64 = conn
            .prepare("SELECT COUNT(*) FROM events WHERE kind = ?1")
            .unwrap()
            .query_row(rusqlite::params![EVENT_KIND_EMAIL_ARCHIVED], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "Only one email_archived event despite two ingests"
        );
    }

    #[test]
    fn delete_archived_blob_blocked_by_fk() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();
        let blob_id = ingest_test_message(&storage, account_id, mailbox_id, "protected", 1);

        // Attempting to delete the blob should fail due to FK from events.
        let conn = storage.open_connection().unwrap();
        let result = conn.execute("DELETE FROM message_blobs WHERE id = ?1", [blob_id]);
        assert!(result.is_err(), "DELETE should be blocked by FK or trigger");
    }

    #[test]
    fn sync_finished_includes_root_hash() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        // Ingest a message first so we have a non-empty root hash.
        ingest_test_message(&storage, account_id, mailbox_id, "for sync", 1);

        // Now create a sync_finished event.
        storage
            .create_sync_finished_event(account_id, "ok", 1, 0)
            .unwrap();

        // Extract the detail JSON from the last sync_finished event.
        let conn = storage.open_connection().unwrap();
        let detail: String = conn
            .prepare("SELECT detail FROM events WHERE kind = ?1 ORDER BY id DESC LIMIT 1")
            .unwrap()
            .query_row(rusqlite::params![EVENT_KIND_SYNC_FINISHED], |r| r.get(0))
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&detail).unwrap();
        assert!(
            parsed.get("root_hash").is_some(),
            "sync_finished should contain root_hash"
        );
        assert!(
            parsed.get("blob_count").is_some(),
            "sync_finished should contain blob_count"
        );
        assert_eq!(
            parsed.get("blob_count").unwrap().as_u64().unwrap(),
            1,
            "blob_count should be 1"
        );
    }

    #[test]
    fn delete_trigger_prevents_blob_removal() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();
        let blob_id = ingest_test_message(&storage, account_id, mailbox_id, "trigger test", 1);

        // Even without the FK event, the BEFORE DELETE trigger should prevent deletion.
        let conn = storage.open_connection().unwrap();
        let result = conn.execute("DELETE FROM message_blobs WHERE id = ?1", [blob_id]);
        assert!(result.is_err(), "DELETE should be blocked by trigger");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not permitted"),
            "Error message should mention 'not permitted', got: {err_msg}"
        );
    }

    #[test]
    fn delete_trigger_prevents_event_removal() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();
        ingest_test_message(&storage, account_id, mailbox_id, "event trigger test", 1);

        // Try to delete events.
        let conn = storage.open_connection().unwrap();
        let result = conn.execute(
            "DELETE FROM events WHERE kind = ?1",
            rusqlite::params![EVENT_KIND_EMAIL_ARCHIVED],
        );
        assert!(result.is_err(), "DELETE should be blocked by trigger");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not permitted"),
            "Error message should mention 'not permitted', got: {err_msg}"
        );
    }

    #[test]
    fn verify_event_chain_detects_deleted_event() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        // Ingest multiple messages to build a chain.
        ingest_test_message(&storage, account_id, mailbox_id, "chain-1", 1);
        ingest_test_message(&storage, account_id, mailbox_id, "chain-2", 2);
        ingest_test_message(&storage, account_id, mailbox_id, "chain-3", 3);

        // The chain should be valid now.
        let result = storage.verify_event_chain().unwrap();
        assert!(
            result.first_mismatch_event_id.is_none(),
            "Chain should be intact"
        );

        // Tamper: bypass trigger via DROP TRIGGER, delete an event, recreate trigger.
        let conn = storage.open_connection().unwrap();
        conn.execute_batch("DROP TRIGGER IF EXISTS prevent_delete_events")
            .unwrap();

        // Find and delete the second email_archived event.
        let second_event_id: i64 = conn
            .prepare("SELECT id FROM events WHERE kind = ?1 ORDER BY id ASC LIMIT 1 OFFSET 1")
            .unwrap()
            .query_row(rusqlite::params![EVENT_KIND_EMAIL_ARCHIVED], |r| r.get(0))
            .unwrap();
        conn.execute("DELETE FROM events WHERE id = ?1", [second_event_id])
            .unwrap();

        // Restore trigger (to stay clean).
        conn.execute_batch(
            r#"
            CREATE TRIGGER IF NOT EXISTS prevent_delete_events
            BEFORE DELETE ON events
            BEGIN
              SELECT RAISE(ABORT, 'Deleting events from the audit log is not permitted.');
            END;
            "#,
        )
        .unwrap();
        drop(conn);

        // Now verification should detect the break.
        let result = storage.verify_event_chain().unwrap();
        assert!(
            result.first_mismatch_event_id.is_some(),
            "Chain should be broken after deleting an event"
        );
    }

    #[test]
    fn root_hash_mismatch_after_blob_deletion() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        ingest_test_message(&storage, account_id, mailbox_id, "rh-1", 1);
        ingest_test_message(&storage, account_id, mailbox_id, "rh-2", 2);

        // Record a sync checkpoint with the current root hash.
        storage
            .create_sync_finished_event(account_id, "ok", 2, 0)
            .unwrap();

        // Full integrity should pass.
        let status = storage.verify_integrity().unwrap();
        assert!(status.ok, "Integrity should be ok before tampering");

        // Tamper: bypass triggers and FK, delete one blob.
        let conn = storage.open_connection().unwrap();
        conn.execute_batch("DROP TRIGGER IF EXISTS prevent_delete_message_blobs")
            .unwrap();
        conn.execute_batch("DROP TRIGGER IF EXISTS prevent_delete_events")
            .unwrap();

        // First, delete events referencing blob 2 (to bypass FK).
        let blob_id_2: i64 = conn
            .prepare("SELECT id FROM message_blobs ORDER BY id DESC LIMIT 1")
            .unwrap()
            .query_row([], |r| r.get(0))
            .unwrap();
        conn.execute("DELETE FROM events WHERE message_blob_id = ?1", [blob_id_2])
            .unwrap();
        conn.execute(
            "DELETE FROM message_locations WHERE message_blob_id = ?1",
            [blob_id_2],
        )
        .unwrap();
        conn.execute("DELETE FROM message_blobs WHERE id = ?1", [blob_id_2])
            .unwrap();

        // Restore triggers.
        conn.execute_batch(
            r#"
            CREATE TRIGGER IF NOT EXISTS prevent_delete_message_blobs
            BEFORE DELETE ON message_blobs
            BEGIN
              SELECT RAISE(ABORT, 'Deleting archived email blobs is not permitted.');
            END;
            CREATE TRIGGER IF NOT EXISTS prevent_delete_events
            BEFORE DELETE ON events
            BEGIN
              SELECT RAISE(ABORT, 'Deleting events from the audit log is not permitted.');
            END;
            "#,
        )
        .unwrap();
        drop(conn);

        // Both full and quick verification should detect the mismatch.
        let full_status = storage.verify_integrity().unwrap();
        assert!(!full_status.ok, "Full integrity should fail");
        assert!(!full_status.root_hash_ok, "Root hash should mismatch");
        assert!(!full_status.issues.is_empty(), "Issues should be reported");

        let quick_status = storage.verify_root_hash_only().unwrap();
        assert!(!quick_status.ok, "Quick check should also fail");
        assert!(
            !quick_status.root_hash_ok,
            "Quick root hash should mismatch"
        );
    }

    #[test]
    fn schema_v2_creates_delete_triggers() {
        let storage = Storage::open_in_memory_for_tests().unwrap();

        // Verify triggers exist.
        let conn = storage.open_connection().unwrap();
        let trigger_count: i64 = conn
            .prepare(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'prevent_delete_%'",
            )
            .unwrap()
            .query_row([], |r| r.get(0))
            .unwrap();
        assert_eq!(trigger_count, 2, "Both delete triggers should exist");
    }

    #[test]
    fn verify_integrity_passes_on_clean_database() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();

        ingest_test_message(&storage, account_id, mailbox_id, "clean-1", 1);
        storage
            .create_sync_finished_event(account_id, "ok", 1, 0)
            .unwrap();

        let status = storage.verify_integrity().unwrap();
        assert!(status.ok);
        assert!(status.chain_ok);
        assert!(status.root_hash_ok);
        assert!(status.issues.is_empty());
    }

    #[test]
    fn verify_root_hash_only_is_ok_without_checkpoint() {
        let (storage, account_id, mailbox_id) = setup_test_account_with_inbox();
        ingest_test_message(&storage, account_id, mailbox_id, "no-checkpoint", 1);

        // No sync_finished event yet → quick check should pass vacuously.
        let status = storage.verify_root_hash_only().unwrap();
        assert!(status.ok, "Should be ok without a checkpoint");
        assert!(status.root_hash_ok);
        assert!(status.checkpoint_root_hash.is_none());
    }
}
