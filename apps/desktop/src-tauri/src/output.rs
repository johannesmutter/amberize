//! Publish complete exports atomically without touching archive files or aliases.
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportState {
    Running,
    Cancelled,
    Published,
    Failed,
}

pub fn check_export(state: Option<&Mutex<ExportState>>) -> io::Result<()> {
    if let Some(state) = state {
        if *state
            .lock()
            .map_err(|_| io::Error::other("Export state unavailable"))?
            == ExportState::Cancelled
        {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Export canceled. Your previous file was kept.",
            ));
        }
    }
    Ok(())
}

/// Return true if publication already won the race with cancellation.
pub fn cancel_export(state: &Mutex<ExportState>) -> Result<bool, String> {
    let mut state = state.lock().map_err(|_| "Export state unavailable")?;
    if *state == ExportState::Published {
        return Ok(true);
    }
    *state = ExportState::Cancelled;
    Ok(false)
}

pub fn validate_destination(archive: &Path, output: &Path) -> io::Result<()> {
    let original = archive.to_path_buf();
    let archive = archive.canonicalize()?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let filename = output
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Choose an export filename"))?;
    let destination = parent.canonicalize()?.join(filename);
    let mut protected = vec![archive.clone()];
    for base in [&archive, &original] {
        for suffix in ["-wal", "-shm", "-journal"] {
            let path = PathBuf::from(format!("{}{suffix}", base.display()));
            let parent = path.parent().unwrap_or(Path::new("."));
            protected.push(parent.canonicalize()?.join(path.file_name().unwrap()));
        }
    }
    for path in protected {
        if destination == path
            || (destination.exists()
                && path.exists()
                && email_archiver_storage::is_same_archive_file(&path, &destination)?)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "The export destination is an archive file. Choose a different filename.",
            ));
        }
    }
    Ok(())
}

pub fn atomic_export<T>(
    archive: &Path,
    output: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<T, String>,
) -> Result<T, String> {
    atomic_export_cancellable(archive, output, write, None)
}

pub fn atomic_export_cancellable<T>(
    archive: &Path,
    output: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<T, String>,
    state: Option<&Mutex<ExportState>>,
) -> Result<T, String> {
    check_export(state).map_err(|e| e.to_string())?;
    validate_destination(archive, output).map_err(|e| e.to_string())?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let result = write(file.as_file_mut())?;
    file.as_file_mut().flush().map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    // Publication must reject a destination changed during export as well.
    validate_destination(archive, output).map_err(|e| e.to_string())?;
    // Hold the same mutex as cancellation across the final rename. A late
    // cancellation then reports Published instead of claiming the file was kept.
    let mut publication = state
        .map(|s| s.lock().map_err(|_| "Export state unavailable"))
        .transpose()?;
    if publication.as_deref() == Some(&ExportState::Cancelled) {
        return Err("Export canceled. Your previous file was kept.".into());
    }
    file.persist(output).map_err(|e| e.to_string())?;
    if let Some(state) = publication.as_deref_mut() {
        *state = ExportState::Published;
    }
    drop(publication);
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    #[ignore = "invoked by the cross-process export lock regression"]
    fn export_exclusive_access_probe() {
        let path = std::env::var_os("AMBERIZE_QA_LOCK_PROBE_PATH").unwrap();
        let conn = rusqlite::Connection::open(PathBuf::from(path)).unwrap();
        conn.busy_timeout(std::time::Duration::from_millis(100))
            .unwrap();
        conn.execute_batch("PRAGMA locking_mode=EXCLUSIVE").unwrap();
        let result = conn.query_row("SELECT COUNT(*) FROM schema_meta", [], |row| {
            row.get::<_, i64>(0)
        });
        assert!(
            matches!(result, Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::DatabaseBusy),
            "archive locks released: {result:?}"
        );
    }
    #[cfg(unix)]
    #[test]
    fn destination_checks_preserve_open_archive_locks() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        let storage =
            email_archiver_storage::Storage::create_new(dir.path().join("archive.db")).unwrap();
        let probe = || {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "output::tests::export_exclusive_access_probe",
                ])
                .env("AMBERIZE_QA_LOCK_PROBE_PATH", storage.db_path())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while child.try_wait().unwrap().is_none() {
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("export lock probe timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        };
        probe();
        let output = dir.path().join("export.zip");
        std::fs::write(&output, b"previous export").unwrap();
        validate_destination(storage.db_path(), &output).unwrap();
        probe();
        let alias = dir.path().join("alias.zip");
        std::fs::hard_link(storage.db_path(), &alias).unwrap();
        assert!(validate_destination(storage.db_path(), &alias).is_err());
        probe();
    }
    #[test]
    fn cancellation_before_publication_preserves_output_and_late_cancel_reports_published() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive.db");
        let output = dir.path().join("export.zip");
        std::fs::write(&archive, b"archive").unwrap();
        std::fs::write(&output, b"previous").unwrap();
        let state = Mutex::new(ExportState::Running);
        let result = atomic_export_cancellable(
            &archive,
            &output,
            |f| {
                f.write_all(b"partial").unwrap();
                assert!(!cancel_export(&state).unwrap());
                Ok(())
            },
            Some(&state),
        );
        assert!(result.unwrap_err().contains("canceled"));
        assert_eq!(std::fs::read(&output).unwrap(), b"previous");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        *state.lock().unwrap() = ExportState::Running;
        atomic_export_cancellable(
            &archive,
            &output,
            |f| f.write_all(b"complete").map_err(|e| e.to_string()),
            Some(&state),
        )
        .unwrap();
        assert!(cancel_export(&state).unwrap());
        assert_eq!(*state.lock().unwrap(), ExportState::Published);
        assert_eq!(std::fs::read(output).unwrap(), b"complete");
    }
    #[test]
    fn rejects_archive_and_hardlink_and_preserves_failed_output() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive.db");
        std::fs::write(&archive, b"archive bytes").unwrap();
        let alias = dir.path().join("alias.zip");
        std::fs::hard_link(&archive, &alias).unwrap();
        for path in [&archive, &alias] {
            assert!(atomic_export(&archive, path, |f| {
                f.write_all(b"bad").unwrap();
                Ok(())
            })
            .is_err());
        }
        let output = dir.path().join("out.zip");
        std::fs::write(&output, b"previous export").unwrap();
        let failed: Result<(), String> = atomic_export(&archive, &output, |f| {
            f.write_all(b"partial").unwrap();
            Err("injected failure".into())
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read(output).unwrap(), b"previous export");
        assert_eq!(std::fs::read(archive).unwrap(), b"archive bytes");
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_and_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive.db");
        std::fs::write(&archive, b"db").unwrap();
        let link = dir.path().join("link.eml");
        std::os::unix::fs::symlink(&archive, &link).unwrap();
        assert!(validate_destination(&archive, &link).is_err());
        assert!(validate_destination(&archive, &dir.path().join("archive.db-wal")).is_err());
    }
}
