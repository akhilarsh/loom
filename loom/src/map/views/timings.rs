//! `--timings`: where one `loom map` invocation spent its time.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Wall time of each phase. `query` covers computing the views and any window
/// or census read; `render` covers assembling the output.
#[derive(Debug, Clone, Copy, Default)]
pub struct Timings {
    pub snapshot: Duration,
    pub load: Duration,
    pub resolve: Duration,
    /// The whole `GraphStore::view` call: `load` when it read a persisted
    /// view, `resolve` when it resolved one. A view `ensure_snapshot` just
    /// materialized is handed over here, so its build time sits in `snapshot`.
    pub view: Duration,
    pub query: Duration,
    pub render: Duration,
}

/// Run `work` and add its wall time to `slot`.
pub fn timed<T>(slot: &mut Duration, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = work();
    *slot += started.elapsed();
    value
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

impl Timings {
    /// The `"timings"` JSON object: every phase in milliseconds, `total` for
    /// the whole invocation, and the process's peak resident set in KiB.
    pub fn to_json(&self, total: Duration) -> Value {
        json!({
            "snapshot": millis(self.snapshot),
            "load": millis(self.load),
            "resolve": millis(self.resolve),
            "view": millis(self.view),
            "query": millis(self.query),
            "render": millis(self.render),
            "total": millis(total),
            "peak_rss_kb": peak_rss_kb(),
        })
    }

    /// The one stderr line `--timings` prints.
    pub fn summary(&self, total: Duration) -> String {
        format!(
            "timings: snapshot {:.1}ms, load {:.1}ms, resolve {:.1}ms, view {:.1}ms, \
             query {:.1}ms, render {:.1}ms, total {:.1}ms, peak_rss {} KiB",
            millis(self.snapshot),
            millis(self.load),
            millis(self.resolve),
            millis(self.view),
            millis(self.query),
            millis(self.render),
            millis(total),
            peak_rss_kb()
        )
    }
}

/// Peak resident set size of this process in KiB, or 0 when the kernel will not
/// say. `ru_maxrss` is KiB on Linux and bytes on macOS.
fn peak_rss_kb() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `getrusage` writes one `rusage` through the pointer, which is
    // valid for the call, and the zeroed value is a valid `rusage` regardless.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if status != 0 {
        return 0;
    }
    // SAFETY: zero-initialised above and, on success, filled in by the kernel.
    let max = u64::try_from(unsafe { usage.assume_init() }.ru_maxrss).unwrap_or(0);
    if cfg!(target_os = "macos") {
        max / 1024
    } else {
        max
    }
}
