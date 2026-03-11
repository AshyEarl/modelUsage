use std::sync::atomic::{AtomicBool, Ordering};

static SUPPRESSED: AtomicBool = AtomicBool::new(false);

pub fn set_suppressed(suppressed: bool) {
    SUPPRESSED.store(suppressed, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    if SUPPRESSED.load(Ordering::Relaxed) {
        return false;
    }
    std::env::var("MODELUSAGE_PROFILE")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub fn log(message: impl AsRef<str>) {
    if SUPPRESSED.load(Ordering::Relaxed) {
        return;
    }
    eprintln!("[profile] {}", message.as_ref());
}

pub fn build_log(message: impl AsRef<str>) {
    if SUPPRESSED.load(Ordering::Relaxed) {
        return;
    }
    eprintln!("[build] {}", message.as_ref());
}
