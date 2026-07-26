use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// User-edit watchdog
// ---------------------------------------------------------------------------

/// Guards the file a task is running against so edits made by the user can be
/// told apart from rik's own writes.
///
/// Only one file is worked on at a time, so a single slot is enough. Every
/// write rik performs (`edit_file`, a dynamic command) calls [`resync`] to
/// adopt the resulting content as its own; anything else that changes the file
/// is the user, and [`changed_externally`] reports it.
#[derive(Debug)]
struct Watched {
    path: PathBuf,
    hash: u64,
}

fn slot() -> &'static Mutex<Option<Watched>> {
    static SLOT: OnceLock<Mutex<Option<Watched>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn lock() -> std::sync::MutexGuard<'static, Option<Watched>> {
    slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Hash the file's current content, or `None` when it cannot be read.
fn hash_of(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    Some(hasher.finish())
}

/// Start guarding `path`, taking its current content as the baseline.
pub fn watch(path: &Path) {
    let watched = hash_of(path).map(|hash| Watched {
        path: path.to_path_buf(),
        hash,
    });
    *lock() = watched;
}

/// Stop guarding whatever file is currently watched.
pub fn stop() {
    *lock() = None;
}

/// Guards `path` for as long as the returned value is alive.
pub fn guard(path: &Path) -> WatchGuard {
    watch(path);
    WatchGuard
}

/// Drops the watch when the task that installed it ends, however it ends.
pub struct WatchGuard;

impl Drop for WatchGuard {
    fn drop(&mut self) {
        stop();
    }
}

/// Adopt the guarded file's current content as rik's own.
///
/// Called after every write rik makes, so its own edits never read as a user
/// modification. Writes to other files leave the guarded file untouched and
/// are therefore harmless.
pub fn resync() {
    let mut guard = lock();
    if let Some(watched) = guard.as_mut()
        && let Some(hash) = hash_of(&watched.path)
    {
        watched.hash = hash;
    }
}

/// Whether the guarded file changed on disk since rik last wrote it.
///
/// A file that became unreadable (deleted, renamed) also counts as changed.
pub fn changed_externally() -> bool {
    let guard = lock();
    let Some(watched) = guard.as_ref() else {
        return false;
    };
    hash_of(&watched.path) != Some(watched.hash)
}

/// Watchdog state is global, so tests that touch it must not run in parallel.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_write_made_outside_rik() -> anyhow::Result<()> {
        let _guard = test_lock();
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        std::fs::write(&file, "rik: do this\n")?;

        watch(&file);
        assert!(!changed_externally());

        std::fs::write(&file, "rik: do this\nuser typed here\n")?;
        assert!(changed_externally());

        stop();
        Ok(())
    }

    #[test]
    fn a_resynced_write_is_not_reported() -> anyhow::Result<()> {
        let _guard = test_lock();
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        std::fs::write(&file, "rik: do this\n")?;

        watch(&file);
        std::fs::write(&file, "done\n")?;
        resync();

        assert!(!changed_externally());
        stop();
        Ok(())
    }

    #[test]
    fn a_write_to_another_file_is_not_reported() -> anyhow::Result<()> {
        let _guard = test_lock();
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let other = dir.path().join("other.txt");
        std::fs::write(&file, "rik: do this\n")?;

        watch(&file);
        std::fs::write(&other, "new file\n")?;
        resync();

        assert!(!changed_externally());
        stop();
        Ok(())
    }

    #[test]
    fn a_deleted_file_counts_as_changed() -> anyhow::Result<()> {
        let _guard = test_lock();
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        std::fs::write(&file, "rik: do this\n")?;

        watch(&file);
        std::fs::remove_file(&file)?;

        assert!(changed_externally());
        stop();
        Ok(())
    }

    #[test]
    fn nothing_is_reported_when_no_file_is_watched() {
        let _guard = test_lock();
        stop();
        assert!(!changed_externally());
    }
}
