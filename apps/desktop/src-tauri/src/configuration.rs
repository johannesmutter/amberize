use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const DEFAULT_SYNC_INTERVAL_SECS: u64 = 300;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub db_path: String,
    #[serde(default = "default_interval")]
    pub sync_interval_secs: u64,
}
pub fn default_interval() -> u64 {
    DEFAULT_SYNC_INTERVAL_SECS
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.previous")
}
fn read_config(path: &Path) -> Result<AppConfig, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let config: AppConfig =
        serde_json::from_slice(&bytes).map_err(|e| format!("Saved settings are damaged: {e}"))?;
    if config.db_path.trim().is_empty() || !(60..=86400).contains(&config.sync_interval_secs) {
        return Err("Saved settings contain an invalid archive path or sync interval".into());
    }
    Ok(config)
}

/// Missing is first run; unreadable/corrupt is a recovery error, never first run.
pub fn load(path: &Path) -> Result<(Option<AppConfig>, Option<String>), String> {
    match read_config(path) {
        Ok(config) => Ok((Some(config), None)),
        Err(error) => {
            if let Ok(config) = read_config(&backup_path(path)) {
                eprintln!("Saved settings recovery: {error}");
                return Ok((
                    Some(config),
                    Some("Amberize restored your previous settings. Review your archive location and sync interval in Settings.".into()),
                ));
            }
            if !path.try_exists().map_err(|e| e.to_string())?
                && !backup_path(path).try_exists().map_err(|e| e.to_string())?
            {
                Ok((None, None))
            } else {
                Err(format!(
                    "Amberize could not restore its saved settings. {error}"
                ))
            }
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Settings path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn save(path: &Path, config: &AppConfig) -> Result<(), String> {
    if config.db_path.trim().is_empty() || !(60..=86400).contains(&config.sync_interval_secs) {
        return Err("Choose a valid archive and sync interval".into());
    }
    if let Ok(previous) = read_config(path) {
        atomic_write(
            &backup_path(path),
            &serde_json::to_vec_pretty(&previous).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    atomic_write(
        path,
        &serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Could not save archive settings: {e}"))
}

pub fn clear(path: &Path) -> Result<(), String> {
    for path in [path.to_path_buf(), backup_path(path)] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_settings_restore_and_corruption_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert!(load(&path).unwrap().0.is_none());
        let config = AppConfig {
            db_path: "/archive.sqlite3".into(),
            sync_interval_secs: 3600,
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap().0.unwrap().sync_interval_secs, 3600);
        save(&path, &config).unwrap();
        std::fs::write(&path, b"{").unwrap();
        let (restored, warning) = load(&path).unwrap();
        assert_eq!(restored.unwrap().db_path, config.db_path);
        assert!(warning.is_some());
        clear(&path).unwrap();
        assert!(load(&path).unwrap().0.is_none());
    }
    #[test]
    fn corrupt_settings_without_backup_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"").unwrap();
        assert!(load(&path).is_err());
    }
    #[test]
    fn legacy_settings_get_a_default_interval() {
        let config: AppConfig = serde_json::from_str(r#"{"db_path":"/archive.sqlite3"}"#).unwrap();
        assert_eq!(config.sync_interval_secs, 300);
    }
}
