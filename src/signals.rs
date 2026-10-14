//! Ctrl+C / SIGTERM handling for long-running commands, so that they stop the
//! head-tracking stream and remove their socket instead of dying mid-flight.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static STOP: AtomicBool = AtomicBool::new(false);
static CLEANUP: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

extern "C" fn on_signal(_: libc::c_int) {
    // Only an atomic store: anything else is not async-signal-safe.
    STOP.store(true, Ordering::SeqCst);
}

pub fn install() {
    // SAFETY: the handler only performs an atomic store.
    unsafe {
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGHUP, handler);
    }
}

pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

pub fn request_stop() {
    STOP.store(true, Ordering::SeqCst);
}

/// Registers a file (our socket) to delete in `cleanup`.
pub fn remove_on_exit(path: PathBuf) {
    CLEANUP.lock().unwrap().push(path);
}

pub fn cleanup() {
    for p in CLEANUP.lock().unwrap().drain(..) {
        let _ = std::fs::remove_file(p);
    }
}
