use anyhow::{Context as _, Result, anyhow};
use rusqlite::{Connection, OpenFlags, backup::{Backup, StepResult}};
use std::{fs, path::Path, time::Duration};

pub fn prepare(destination: &Path, legacy: &[&Path]) -> Result<()> {
    if destination.is_dir() {
        return Ok(());
    }
    let parent = destination.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new().prefix(".xdytube-profile-").tempdir_in(parent)?;
    for source in legacy {
        let settings = source.join("settings.json");
        let database = [source.join("streamtube.db"), source.join("ferrotube.db")]
            .into_iter().find(|path| path.is_file());
        if database.is_none() && !settings.is_file() {
            continue;
        }
        if let Some(database) = database {
            copy_database(&database, &staging.path().join("streamtube.db"))?;
        }
        if settings.is_file() {
            fs::copy(settings, staging.path().join("settings.json"))?;
        }
        break;
    }
    match fs::rename(staging.path(), destination) {
        Ok(()) => Ok(()),
        Err(_) if destination.is_dir() => Ok(()),
        Err(error) => Err(error).context("could not activate the xdytube profile"),
    }
}

fn copy_database(source: &Path, destination: &Path) -> Result<()> {
    let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    source.busy_timeout(Duration::from_secs(2))?;
    let mut snapshot = Connection::open_in_memory()?;
    {
        let backup = Backup::new(&source, &mut snapshot)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if matches!(backup.step(-1)?, StepResult::Done) {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(anyhow!("the previous profile is busy; close Stream Tube and try again"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let has_kv: bool = snapshot.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='kv')", [], |row| row.get(0),
    )?;
    if has_kv {
        snapshot.execute("DELETE FROM kv WHERE key IN ('auth', 'web_session')", [])?;
    }
    // Rebuilding into a fresh file keeps removed rows out of the new database's free pages.
    let path = destination.to_str().context("the new database path is not valid UTF-8")?;
    snapshot.execute("VACUUM INTO ?1", [path])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_preserves_wal_data_and_leaves_credentials_with_the_old_profile() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("StreamTube");
        let new = root.path().join("xdytube");
        fs::create_dir(&old).unwrap();
        fs::write(old.join("settings.json"), br#"{"theme":"dark"}"#).unwrap();
        fs::write(old.join("instance"), "local test").unwrap();
        let from = Connection::open(old.join("streamtube.db")).unwrap();
        from.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE history (progress INTEGER);
            INSERT INTO kv VALUES ('auth', 'test-old-auth'), ('web_session', 'test-old-web'), ('settings', 'keep');
            INSERT INTO history VALUES (120);").unwrap();

        prepare(&new, &[&old]).unwrap();
        assert_eq!(fs::read(new.join("settings.json")).unwrap(), br#"{"theme":"dark"}"#);
        assert!(!new.join("instance").exists());
        let to = Connection::open(new.join("streamtube.db")).unwrap();
        assert_eq!(to.query_row("SELECT progress FROM history", [], |row| row.get::<_, i32>(0)).unwrap(), 120);
        assert_eq!(to.query_row("SELECT key FROM kv", [], |row| row.get::<_, String>(0)).unwrap(), "settings");
        assert_eq!(to.query_row("SELECT count(*) FROM kv", [], |row| row.get::<_, i32>(0)).unwrap(), 1);
        assert_eq!(from.query_row("SELECT count(*) FROM kv", [], |row| row.get::<_, i32>(0)).unwrap(), 3);
        let bytes = fs::read(new.join("streamtube.db")).unwrap();
        assert!(!bytes.windows(13).any(|window| window == b"test-old-auth"));
        to.execute("UPDATE history SET progress=240", []).unwrap();
        prepare(&new, &[&old]).unwrap();
        assert_eq!(to.query_row("SELECT progress FROM history", [], |row| row.get::<_, i32>(0)).unwrap(), 240);
    }

    #[test]
    fn a_failed_migration_does_not_activate_an_empty_profile() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("StreamTube");
        let new = root.path().join("xdytube");
        fs::create_dir(&old).unwrap();
        fs::write(old.join("streamtube.db"), b"invalid database").unwrap();
        assert!(prepare(&new, &[&old]).is_err());
        assert!(!new.exists());
    }
}
