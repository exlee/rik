use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Guards a single file against partial edits on cancellation.
///
/// On construction the original file contents are saved with every `{alias}:`
/// replaced by `!{alias}:` so that if we revert, the guard tag will prevent
/// rik from re-entering the same file immediately.
///
/// Uses an atomic bool for success — no mutex needed. Safe behind `Arc`.
pub struct FileReverter {
    file_path: PathBuf,
    original_content: String,
    success: AtomicBool,
}

impl FileReverter {
    /// Create a new guard for `file_path` and register it globally.
    ///
    /// Returns `None` if the file cannot be read.
    pub fn new(file_path: &Path, alias: &str) -> Option<std::sync::Arc<Self>> {
        let content = std::fs::read_to_string(file_path).ok()?;
        // Guard the restored version so rik won't re-process after a revert.
        let guarded = content.replace(&format!("{alias}:"), &format!("!{alias}:"));

        let arc = std::sync::Arc::new(Self {
            file_path: file_path.to_path_buf(),
            original_content: guarded,
            success: AtomicBool::new(false),
        });

        crate::cleanup::register(arc.clone());
        Some(arc)
    }

    /// Mark this edit batch as successful — suppress revert.
    pub fn mark_success(&self) {
        self.success.store(true, Ordering::Relaxed);
    }

    /// Write backed-up content back to disk only if not yet marked successful.
    pub fn revert_if_needed(&self) {
        if !self.success.load(Ordering::Relaxed) {
            println!("[cancel]: reverting {}", self.file_path.display());
            if let Err(e) = std::fs::write(&self.file_path, &self.original_content) {
                eprintln!(
                    "CRITICAL: failed to revert {}: {}",
                    self.file_path.display(),
                    e
                );
            }
        }
    }
}

impl Drop for FileReverter {
    fn drop(&mut self) {
        self.revert_if_needed();
    }
}
