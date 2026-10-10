//! Per-application network bandwidth ("which app is using the network now").
//!
//! macOS exposes per-process network byte counters only through the private
//! `nstat` API. The supported, entitlement-free way to reach the same kernel
//! accounting is Apple's own `nettop(1)` tool, which ships with every macOS
//! and reads those counters for us. We run it in its machine-readable mode
//!
//! ```text
//! nettop -P -x -J bytes_in,bytes_out -t wifi -t wired -L 1
//! ```
//!
//! (-P per-process, -x CSV, -J only the byte columns, -t external
//! interfaces only, -L 1 single sample) on a background thread so the UI
//! never blocks, parse the CSV, and turn the cumulative per-process byte
//! counters into per-second rates from consecutive samples.
//!
//! Honesty notes (surfaced in the UI):
//! - rates are approximate: nettop samples on its own ~1 s cadence and
//!   attributes traffic to the process that owns the socket;
//! - loopback-only traffic is excluded on purpose (-t wifi -t wired);
//! - on non-macOS platforms there is no supported per-process network
//!   counter source, so the sampler reports itself unavailable instead of
//!   guessing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

/// Live bandwidth of one process.
#[derive(Clone, Debug, Default)]
pub struct ProcBandwidth {
    pub pid: i32,
    pub name: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
    /// Cumulative bytes since the process opened its sockets (nettop view).
    pub total_rx: u64,
    pub total_tx: u64,
}

/// Whether the platform can supply per-app bandwidth at all.
pub fn platform_supported() -> bool {
    cfg!(target_os = "macos")
}

// ---------------------------------------------------------------- parsing

/// One raw nettop CSV row: process identity + cumulative counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NettopRow {
    pub pid: i32,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

/// Split one CSV line, honoring double-quoted fields ("" = literal quote).
fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_q && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => in_q = !in_q,
            ',' if !in_q => {
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Parse a byte count that is either a plain integer (`12345`) or carries a
/// human suffix (`12K`, `3.4M`, `1.2G`) — nettop emits plain integers in -x
/// mode, but being tolerant costs nothing.
fn parse_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (digits, mult) = match s.chars().last()? {
        'K' | 'k' => (&s[..s.len() - 1], 1_024.0),
        'M' | 'm' => (&s[..s.len() - 1], 1_048_576.0),
        'G' | 'g' => (&s[..s.len() - 1], 1_073_741_824.0),
        'B' if s.len() > 1 => (&s[..s.len() - 1], 1.0),
        _ => (s, 1.0),
    };
    let v: f64 = digits.trim().parse().ok()?;
    if v < 0.0 {
        return None;
    }
    Some((v * mult) as u64)
}

/// Parse nettop's `-P -x -J bytes_in,bytes_out` output.
///
/// Shape (verified against macOS 13–15):
/// ```text
/// ,bytes_in,bytes_out,
/// Time,,,
/// Safari.501,2202014,183042,
/// "Google Chrome Helper (Renderer)".512,883012,44102,
/// ```
/// The first row is the column header (first cell empty), `Time` rows mark
/// sample boundaries, and data rows are `name.pid`. Names may contain dots
/// and commas (quoted), so the pid is split off at the last dot.
pub fn parse_nettop_csv(text: &str) -> Vec<(String, NettopRow)> {
    let mut idx_in = 1usize;
    let mut idx_out = 2usize;
    let mut saw_header = false;
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let f = csv_fields(line);
        if f.len() < 2 {
            continue;
        }
        let first = f[0].trim();
        if first.is_empty() {
            // header row: locate byte columns by name if present
            let li = f
                .iter()
                .position(|c| c.trim().eq_ignore_ascii_case("bytes_in"));
            let lo = f
                .iter()
                .position(|c| c.trim().eq_ignore_ascii_case("bytes_out"));
            if let (Some(a), Some(b)) = (li, lo) {
                idx_in = a;
                idx_out = b;
                saw_header = true;
            }
            continue;
        }
        if first.eq_ignore_ascii_case("time") {
            continue;
        }
        // name.pid — pid is the numeric suffix after the last '.'
        let Some(dot) = first.rfind('.') else {
            continue;
        };
        let name = &first[..dot];
        let Ok(pid) = first[dot + 1..].trim().parse::<i32>() else {
            continue;
        };
        if name.is_empty() || pid < 0 {
            continue;
        }
        let (Some(bi), Some(bo)) = (
            f.get(idx_in).and_then(|s| parse_bytes(s)),
            f.get(idx_out).and_then(|s| parse_bytes(s)),
        ) else {
            continue;
        };
        let _ = saw_header;
        rows.push((
            name.to_string(),
            NettopRow {
                pid,
                bytes_in: bi,
                bytes_out: bo,
            },
        ));
    }
    rows
}

// ----------------------------------------------------------- rate math

/// Cumulative counters of the previous sample, keyed by pid.
pub type PrevSample = HashMap<i32, (String, u64, u64)>;

/// Turn two cumulative samples + elapsed time into per-process rates.
/// Pure (unit-tested on every platform). Handles counter resets from pid
/// reuse or interface flaps by clamping negative deltas to zero.
pub fn rates_between(
    prev: &PrevSample,
    cur: &[(String, NettopRow)],
    dt: f64,
) -> (Vec<ProcBandwidth>, PrevSample) {
    let mut out = Vec::with_capacity(cur.len());
    let mut next: PrevSample = HashMap::with_capacity(cur.len());
    let dt = dt.max(0.05);
    for (name, row) in cur {
        let (rx_rate, tx_rate) = match prev.get(&row.pid) {
            Some((_, pi, po)) => (
                row.bytes_in.saturating_sub(*pi) as f64 / dt,
                row.bytes_out.saturating_sub(*po) as f64 / dt,
            ),
            // first sighting: no honest rate yet
            None => (0.0, 0.0),
        };
        next.insert(row.pid, (name.clone(), row.bytes_in, row.bytes_out));
        out.push(ProcBandwidth {
            pid: row.pid,
            name: name.clone(),
            rx_bps: rx_rate,
            tx_bps: tx_rate,
            total_rx: row.bytes_in,
            total_tx: row.bytes_out,
        });
    }
    out.sort_by(|a, b| {
        (b.rx_bps + b.tx_bps)
            .partial_cmp(&(a.rx_bps + a.tx_bps))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    (out, next)
}

// ------------------------------------------------------------- sampler

struct Shared {
    rates: Vec<ProcBandwidth>,
    /// human note when the sampler is unavailable/degraded ("" = healthy)
    note: String,
    ready: bool,
}

/// Background nettop sampler. Construct once per process; the worker thread
/// starts on first use. `latest()` never blocks the caller.
pub struct BandwidthSampler {
    shared: Arc<Mutex<Shared>>,
}

static SAMPLER: OnceLock<BandwidthSampler> = OnceLock::new();

/// Process-global sampler (one nettop pipeline for the whole app).
pub fn global_sampler() -> &'static BandwidthSampler {
    SAMPLER.get_or_init(BandwidthSampler::start)
}

impl BandwidthSampler {
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            rates: Vec::new(),
            note: String::new(),
            ready: false,
        }));
        #[cfg(target_os = "macos")]
        {
            let s2 = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("nettop-sampler".into())
                .spawn(move || sampler_loop(s2))
                .ok();
        }
        #[cfg(not(target_os = "macos"))]
        {
            shared.lock().unwrap().note =
                "per-app rates need macOS nettop (no supported source on this platform)".into();
        }
        BandwidthSampler { shared }
    }

    /// Most recent per-process rates (empty until the first sample pair).
    pub fn latest(&self) -> Vec<ProcBandwidth> {
        self.shared
            .lock()
            .map(|s| s.rates.clone())
            .unwrap_or_default()
    }

    /// "" when sampling is healthy, otherwise why rates are unavailable.
    pub fn note(&self) -> String {
        self.shared
            .lock()
            .map(|s| s.note.clone())
            .unwrap_or_else(|_| "sampler lock poisoned".into())
    }

    /// True once at least one rate set has been published.
    pub fn ready(&self) -> bool {
        self.shared.lock().map(|s| s.ready).unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
fn sampler_loop(shared: Arc<Mutex<Shared>>) {
    let mut prev: PrevSample = HashMap::new();
    let mut prev_at = Instant::now();
    let mut failures = 0u32;
    loop {
        let started = Instant::now();
        match run_nettop_once() {
            Some(rows) => {
                failures = 0;
                let dt = started.duration_since(prev_at);
                let (rates, next) = rates_between(&prev, &rows, dt.as_secs_f64());
                prev = next;
                prev_at = started;
                if let Ok(mut s) = shared.lock() {
                    s.rates = rates;
                    s.note = String::new();
                    s.ready = true;
                }
            }
            None => {
                failures += 1;
                if let Ok(mut s) = shared.lock() {
                    s.note = if failures >= 3 {
                        format!("nettop unavailable ({} consecutive failures)", failures)
                    } else {
                        "nettop sample failed, retrying".into()
                    };
                }
                if failures >= 3 {
                    // back off hard rather than spinning on a broken tool
                    std::thread::sleep(Duration::from_secs(30));
                }
            }
        }
        // nettop -L 1 returns after its own sample interval; keep roughly a
        // 1 s cadence so long-running samples don't busy-loop
        let elapsed = started.elapsed();
        if elapsed < Duration::from_millis(1000) {
            std::thread::sleep(Duration::from_millis(1000) - elapsed);
        }
    }
}

#[cfg(target_os = "macos")]
fn run_nettop_once() -> Option<Vec<(String, NettopRow)>> {
    let out = std::process::Command::new("/usr/bin/nettop")
        .args([
            "-P",
            "-x",
            "-J",
            "bytes_in,bytes_out",
            "-t",
            "wifi",
            "-t",
            "wired",
            "-L",
            "1",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(parse_nettop_csv(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
,bytes_in,bytes_out,
Time,,,
Safari.501,2202014,183042,
\"Google Chrome Helper (Renderer)\".512,883012,44102,
com.apple.WebKit.Networking.488,0,0,
,,,
mDNSResponder.88,11019853,3118843,
";

    #[test]
    fn parses_real_nettop_shape() {
        let rows = parse_nettop_csv(SAMPLE);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].0, "Safari");
        assert_eq!(
            rows[0].1,
            NettopRow {
                pid: 501,
                bytes_in: 2_202_014,
                bytes_out: 183_042
            }
        );
        // quoted name with spaces/parens, comma-free
        assert_eq!(rows[1].0, "Google Chrome Helper (Renderer)");
        assert_eq!(rows[1].1.pid, 512);
        // zero rows kept (a process can legitimately sit at 0)
        assert_eq!(rows[2].0, "com.apple.WebKit.Networking");
        // the ",,," junk row is skipped
        assert_eq!(rows[3].1.pid, 88);
    }

    #[test]
    fn quoted_name_with_comma_and_dots() {
        let rows = parse_nettop_csv(",bytes_in,bytes_out,\n\"Acme, Inc. Helper.v2\".77,10,20,\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "Acme, Inc. Helper.v2");
        assert_eq!(rows[0].1.pid, 77);
    }

    #[test]
    fn suffix_values_accepted() {
        assert_eq!(parse_bytes("2048"), Some(2048));
        assert_eq!(parse_bytes("2K"), Some(2048));
        assert_eq!(parse_bytes("1.5M"), Some(1_572_864));
        assert_eq!(parse_bytes(""), None);
        assert_eq!(parse_bytes("-3"), None);
    }

    #[test]
    fn rates_from_consecutive_samples() {
        let mut prev: PrevSample = HashMap::new();
        prev.insert(1, ("a".into(), 1000, 2000));
        let cur = vec![
            (
                "a".into(),
                NettopRow {
                    pid: 1,
                    bytes_in: 3000,
                    bytes_out: 2000,
                },
            ),
            (
                "b".into(),
                NettopRow {
                    pid: 2,
                    bytes_in: 500,
                    bytes_out: 100,
                },
            ),
        ];
        let (rates, next) = rates_between(&prev, &cur, 2.0);
        let a = rates.iter().find(|r| r.pid == 1).unwrap();
        assert_eq!(a.rx_bps, 1000.0); // (3000-1000)/2s
        assert_eq!(a.tx_bps, 0.0);
        let b = rates.iter().find(|r| r.pid == 2).unwrap();
        assert_eq!(b.rx_bps, 0.0); // first sighting: honest zero
        assert!(next.contains_key(&2));
    }

    #[test]
    fn counter_reset_clamps_to_zero() {
        let mut prev: PrevSample = HashMap::new();
        prev.insert(9, ("x".into(), 5000, 5000));
        let cur = vec![(
            "x".into(),
            NettopRow {
                pid: 9,
                bytes_in: 100,
                bytes_out: 50,
            },
        )];
        let (rates, _) = rates_between(&prev, &cur, 1.0);
        assert_eq!(rates[0].rx_bps, 0.0);
        assert_eq!(rates[0].tx_bps, 0.0);
    }

    #[test]
    fn csv_fields_quotes() {
        assert_eq!(csv_fields("a,b,c"), vec!["a", "b", "c"]);
        assert_eq!(csv_fields("\"a,b\",\"c\"\"d\",e"), vec!["a,b", "c\"d", "e"]);
        assert_eq!(csv_fields(",,,"), vec!["", "", "", ""]);
    }
}
