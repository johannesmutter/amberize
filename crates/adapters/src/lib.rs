//! Side-effect adapters for Amberize.
//!
//! IMAP, Keychain, filesystem exports, clocks, and OAuth live here behind traits.

pub mod imap;
pub mod oauth;
mod secrets;
mod sync;

pub use secrets::{KeychainSecretStore, MemorySecretStore, SecretStore, SecretStoreError};
pub use sync::is_hard_excluded_by_common_name;
pub use sync::{
    sync_account_once, sync_account_once_with_progress, SyncError, SyncProgress, SyncProgressFn,
    SyncSummary,
};
