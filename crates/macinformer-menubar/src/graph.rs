//! Pure presentation logic for the menu-bar shell.
//!
//! Everything here is platform-free and unit-tested on every OS; the macOS
//! Cocoa layer (app.rs) only draws what these functions compute. Kept in a
//! separate module so `cargo test` on Linux exercises 100% of the layout
//! and formatting decisions the menu bar will show.

use macinformer_core::{human_bytes, human_rate, BatteryInfo, ProcessInfo, SensorInfo, Snapshot};

/// Resample the tail of `values` into `n` bar heights (0..=height), scaled
/// against `max`. Fewer values than bars: bars align right (newest last),
/// leading bars are 0 — same left-to-right time axis as SI's tray graphs.
pub fn bars_from(values: &[f32], max: f32, n: usize, height: usize) -> Vec<usize> {
    let max = max.max(1.0);
    let mut out = vec![0usize; n];
    let take = values.len().min(n);
    for (i, v) in values[values.len() - take..].iter().enumerate() {
        let slot = n - take + i;
        let h = ((*v).clamp(0.0, max) / max * height as f32).round() as usize;
        out[slot] = h.min(height);
    }
    out
}

/// Compact status-item title shown next to the graph ("42%").
pub fn title_for(cpu: f32) -> String {
    format!("{:.0}%", cpu.clamp(0.0, 999.0))
}

pub fn line_cpu(s: &Snapshot) -> String {
    format!("CPU: {:.1}%  ({} cores, load {:.2})", s.cpu.total, s.cpu.logical_count, s.cpu.load_avg[0])
}

pub fn line_memory(s: &Snapshot) -> String {
    let pct = if s.memory.total > 0 { s.memory.used as f64 / s.memory.total as f64 * 100.0 } else { 0.0 };
    format!("Memory: {} of {} ({:.0}%)", human_bytes(s.memory.used), human_bytes(s.memory.total), pct)
}

pub fn line_gpu(s: &Snapshot) -> String {
    match &s.gpu {
        Some(g) => {
            let mut l = format!("GPU: {:.1}%", g.utilization);
            if let Some(w) = g.power_watts {
                l.push_str(&format!("  ({w:.2} W)"));
            }
            l
        }
        None => "GPU: n/a on this platform".into(),
    }
}

pub fn line_sensors(se: &SensorInfo) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(t) = se.cpu_temp_c {
        parts.push(format!("CPU {t:.0}°C"));
    }
    if let Some(t) = se.gpu_temp_c {
        parts.push(format!("GPU {t:.0}°C"));
    }
    if let Some(t) = se.ssd_temp_c {
        parts.push(format!("SSD {t:.0}°C"));
    }
    if let Some(f) = se.fan_rpm {
        parts.push(format!("fan {f:.0} rpm"));
    }
    if parts.is_empty() {
        "Sensors: unavailable".into()
    } else {
        format!("Temp: {}", parts.join("  "))
    }
}

pub fn line_battery(b: &BatteryInfo) -> String {
    if !b.present {
        return "Battery: none (desktop Mac)".into();
    }
    let state = if b.charging { "charging" } else if b.on_ac { "on AC" } else { "discharging" };
    let mut l = format!("Battery: {:.0}% ({state})", b.percent);
    if let Some(m) = b.minutes {
        l.push_str(&format!("  {}:{:02}", m / 60, m % 60));
    }
    l
}

pub fn line_network(s: &Snapshot) -> String {
    let (mut rx, mut tx) = (0.0, 0.0);
    for n in &s.networks {
        if n.is_up {
            rx += n.rx_bps;
            tx += n.tx_bps;
        }
    }
    format!("Network: ↓ {}  ↑ {}", human_rate(rx), human_rate(tx))
}

/// Top-`n` live processes by CPU as menu lines ("1. Safari — 12.3%").
pub fn lines_top(procs: &[ProcessInfo], n: usize) -> Vec<String> {
    let mut live: Vec<&ProcessInfo> = procs.iter().filter(|p| !p.is_dead).collect();
    live.sort_by(|a, b| b.cpu_percent.partial_cmp(&a.cpu_percent).unwrap_or(std::cmp::Ordering::Equal));
    live.iter()
        .take(n)
        .enumerate()
        .map(|(i, p)| format!("{}. {} — {:.1}%", i + 1, p.name, p.cpu_percent))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_scale_and_align_right() {
        let v = [10.0f32, 50.0, 100.0];
        let b = bars_from(&v, 100.0, 5, 8);
        assert_eq!(b, vec![0, 0, 1, 4, 8]); // newest last, zero-padded left
    }

    #[test]
    fn bars_clamp_over_max() {
        let b = bars_from(&[250.0], 100.0, 3, 10);
        assert_eq!(b, vec![0, 0, 10]);
    }

    #[test]
    fn bars_take_last_n() {
        let v: Vec<f32> = (0..20).map(|i| i as f32).collect();
        let b = bars_from(&v, 100.0, 4, 10);
        assert_eq!(b.len(), 4);
        assert_eq!(b[3], 2); // 19/100*10 ≈ 2
        assert_eq!(b[0], 2); // 16/100*10 ≈ 2
    }

    #[test]
    fn bars_empty_history() {
        assert_eq!(bars_from(&[], 100.0, 4, 10), vec![0, 0, 0, 0]);
    }

    #[test]
    fn title_rounds() {
        assert_eq!(title_for(42.4), "42%");
        assert_eq!(title_for(42.6), "43%");
        assert_eq!(title_for(0.0), "0%");
    }

    #[test]
    fn lines_format() {
        let mut s = Snapshot::default();
        s.cpu.total = 12.34;
        s.cpu.logical_count = 8;
        s.memory.total = 16 * 1024 * 1024 * 1024;
        s.memory.used = 8 * 1024 * 1024 * 1024;
        assert!(line_cpu(&s).starts_with("CPU: 12.3%"));
        assert!(line_memory(&s).contains("50%"));
        assert_eq!(line_gpu(&s), "GPU: n/a on this platform");
        assert!(line_network(&s).starts_with("Network: ↓ 0  ↑ 0"));
        let b = BatteryInfo { present: true, percent: 87.0, charging: false, on_ac: true, ..Default::default() };
        assert!(line_battery(&b).contains("87%"));
        assert_eq!(line_battery(&BatteryInfo::default()), "Battery: none (desktop Mac)");
    }

    #[test]
    fn top_lines_sorted() {
        let mk = |name: &str, cpu: f32, dead: bool| ProcessInfo { name: name.into(), cpu_percent: cpu, is_dead: dead, ..Default::default() };
        let procs = vec![mk("a", 5.0, false), mk("b", 50.0, false), mk("c", 30.0, true)];
        let lines = lines_top(&procs, 3);
        assert_eq!(lines.len(), 2); // dead process excluded
        assert!(lines[0].starts_with("1. b — 50.0%"));
        assert!(lines[1].starts_with("2. a — 5.0%"));
    }
}
