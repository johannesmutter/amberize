//! Core logic for Amberize.
//!
//! This crate is designed to be pure and deterministic. Side effects (IMAP, Keychain,
//! filesystem, time) live in adapters and are injected through traits.
