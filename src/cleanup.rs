use std::sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, Ordering}};

use crate::raii::FileReverter;

#[derive(Default)]
pub struct CleanupHandler {
    pub file_reverters: Vec<Weak<FileReverter>>,
}

pub static CLEANUP: OnceLock<Mutex<CleanupHandler>> = OnceLock::new();

fn get() -> &'static Mutex<CleanupHandler> {
    CLEANUP.get_or_init(|| Mutex::new(CleanupHandler::default()))
}

/// Register a reverter so it runs on Ctrl+C.
/// Keeps a strong Arc — the registry owns the lifetime.
pub fn register(reverter: Arc<FileReverter>) {
    get().lock().unwrap().file_reverters.push(Arc::downgrade(&reverter));
}


/// Run all registered reverters that haven't marked success, then clear.
pub fn cleanup() {
    SHUTDOWN.store(true, Ordering::SeqCst);
    let guard = match get().lock() {
        Ok(g) => g,
        Err(_) => {
            eprintln!("Cleanup: could not acquire lock");
            return;
        }
    };

    for arc in guard.file_reverters.iter().filter_map(|r| r.upgrade()) {
        arc.revert_if_needed();
    }
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
pub fn is_shutting_down() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}
