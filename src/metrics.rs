//! Optional performance metrics, enabled via QUICKDIFF_METRICS=1.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static METRICS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize metrics from environment. Call once at startup.
pub fn init() {
    let enabled = std::env::var("QUICKDIFF_METRICS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    METRICS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if metrics collection is enabled.
#[inline]
pub fn enabled() -> bool {
    METRICS_ENABLED.load(Ordering::Relaxed)
}

/// RAII timer that logs duration on drop.
pub struct Timer {
    label: &'static str,
    start: Instant,
}

impl Timer {
    /// Start a timer if metrics are enabled.
    #[inline]
    pub fn start(label: &'static str) -> Option<Self> {
        if enabled() {
            Some(Self {
                label,
                start: Instant::now(),
            })
        } else {
            None
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        log_metric(self.label, elapsed);
    }
}

/// Log a metric to stderr.
fn log_metric(label: &str, duration: Duration) {
    eprintln!("[metrics] {}: {:?}", label, duration);
}
