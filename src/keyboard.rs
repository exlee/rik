use std::sync::{Arc, OnceLock, RwLock};

#[derive(Default)]
pub struct StopStatus {
    stop: bool,
    soft: bool,
}
pub type StopStatusRef = Arc<RwLock<StopStatus>>;
static STOP: OnceLock<Arc<RwLock<StopStatus>>> = OnceLock::new();

fn stop() -> StopStatusRef {
    STOP.get_or_init(|| {
    let stop = StopStatus::default();
    Arc::new(RwLock::new(stop))
    }).clone()
}
pub fn set_stop() {
    let stop = stop();
    let mut stop_status = stop.write().expect("Can't acquire stop lock");
    stop_status.stop = true;
}
pub fn set_soft_stop() {
    let stop = stop();
    let mut stop_status = stop.write().expect("Can't acquire stop lock");
    stop_status.stop = true;
    stop_status.soft = true;
}

pub fn should_stop() -> bool {
    let stop = stop();
    stop.read().map(|v| v.stop).unwrap_or_default()
}

pub fn is_soft_stop() -> bool {
    let stop = stop();
    stop.read().map(|v| v.stop && v.soft).unwrap_or_default()
}

pub fn clear_stop() {
    let stop = stop();
    let mut stop_status = stop.write().expect("Can't acquire stop lock");
    stop_status.stop = false;
    stop_status.soft = false;
}

/// Check if Space was pressed on stdin using non-canonical (raw) mode.
///
/// Briefly sets stdin to non-canonical + non-blocking, reads pending bytes,
/// then immediately restores the original termios.
#[cfg(unix)]
pub fn poll_space_key() -> bool {
    let fd = 0;
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut termios) } != 0 {
        return false;
    }
    let original = termios;

    // Non-canonical, no echo, non-blocking.
    termios.c_lflag &= !(libc::ICANON | libc::ECHO);
    termios.c_cc[libc::VMIN] = 0;
    termios.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } != 0 {
        return false;
    }

    let mut buf = [0u8; 32];
    let space_pressed = match unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) } {
        n if n > 0 => buf[..n as usize].contains(&b' '),
        _ => false,
    };

    // Always restore original settings.
    unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };

    if space_pressed {
        eprintln!("\nStopped.");
    }

    space_pressed
}

/// No-op on non-Unix (Windows) -- no termios support.
#[cfg(not(unix))]
pub fn poll_space_key() -> bool {
    false
}

/// Spawn a background thread that polls for Space key every 100ms.
/// Sets the internal stop flag when detected. After the flag is cleared
/// (via `clear_stop()`), the listener automatically resumes polling.
/// Exits only when `cleanup::is_shutting_down()` becomes true.
pub fn start_space_listener() {
    tokio::task::spawn_blocking(|| {
        loop {
            if crate::cleanup::is_shutting_down() {
                break;
            }
            if poll_space_key() {
                set_stop();
                // Spin until the flag is cleared, then resume listening.
                while !crate::cleanup::is_shutting_down() && should_stop() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}
