pub mod format;
pub mod osc;
pub mod scroll;

use std::sync::atomic::{AtomicBool, Ordering};

static VRCHAT_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    VRCHAT_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    VRCHAT_ENABLED.load(Ordering::Relaxed)
}
