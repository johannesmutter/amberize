//! Cross-process persistence check using a caller-generated, unique synthetic secret reference.
use email_archiver_adapters::{KeychainSecretStore, SecretStore};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let reference = args
        .get(2)
        .ok_or("Usage: qa_keychain write|read|delete qa/UNIQUE-ID")?;
    if !reference.starts_with("qa/") || reference.len() < 12 {
        return Err("Only unique synthetic QA references are accepted".into());
    }
    let store = KeychainSecretStore::new();
    const SYNTHETIC: &str = "amberize-synthetic-qa-credential-v1";
    match args.get(1).map(String::as_str) {
        Some("write") => {
            if store.get_secret(reference)?.is_some() {
                return Err("Refusing to overwrite an existing credential".into());
            }
            store.set_secret(reference, SYNTHETIC)?;
        }
        Some("read") => {
            if store.get_secret(reference)?.as_deref() != Some(SYNTHETIC) {
                return Err("Synthetic credential did not persist across processes".into());
            }
        }
        Some("delete") => {
            if store.get_secret(reference)?.as_deref() != Some(SYNTHETIC) {
                return Err(
                    "Refusing to delete a credential that is not the synthetic fixture".into(),
                );
            }
            store.delete_secret(reference)?;
        }
        _ => return Err("Usage: qa_keychain write|read|delete qa/UNIQUE-ID".into()),
    }
    println!("{} synthetic credential: passed", args[1]);
    Ok(())
}
