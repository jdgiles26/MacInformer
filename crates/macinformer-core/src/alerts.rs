//! Threshold alerts — the "rules engine" category that tools like iStat Menus
//! charge for: warn when CPU runs hot, memory fills up, a disk is nearly
//! full, a temperature crosses a line, or a named process disappears.
//!
//! Rules are fixed, sensible defaults plus user-tweakable thresholds (keys in
//! the TUI). Evaluation is pure and fully unit-tested; notification delivery
//! (status bar / macOS notification) is the caller's job.

use crate::sensors::{BatteryInfo, SensorInfo};
use crate::types::Snapshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Cpu,
    Memory,
    DiskFull,
    Temperature,
    ProcessGone,
    BatteryLow,
}

impl AlertKind {
    pub fn label(self) -> &'static str {
        match self {
            AlertKind::Cpu => "CPU",
            AlertKind::Memory => "Memory",
            AlertKind::DiskFull => "Disk",
            AlertKind::Temperature => "Temperature",
            AlertKind::ProcessGone => "Process",
            AlertKind::BatteryLow => "Battery",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Alert {
    pub kind: AlertKind,
    pub message: String,
}

/// User-adjustable thresholds (percent / °C).
#[derive(Clone, Debug)]
pub struct AlertConfig {
    pub enabled: bool,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub disk_free_percent: f64,
    pub temp_c: f64,
    pub battery_percent: f64,
    /// Process names that must be running (empty = rule off).
    pub watch_processes: Vec<String>,
}

impl Default for AlertConfig {
    fn default() -> Self {
        AlertConfig {
            enabled: true,
            cpu_percent: 90.0,
            memory_percent: 90.0,
            disk_free_percent: 5.0,
            temp_c: 95.0,
            battery_percent: 15.0,
            watch_processes: Vec::new(),
        }
    }
}

/// Evaluate every rule against the current snapshot. Pure: no side effects,
/// returns the full set of *currently violated* conditions (the tracker
/// below decides when to *raise*).
pub fn evaluate(cfg: &AlertConfig, snap: &Snapshot, sensors: &SensorInfo, battery: &BatteryInfo) -> Vec<Alert> {
    if !cfg.enabled {
        return Vec::new();
    }
    let mut out = Vec::new();
    if snap.cpu.total as f64 >= cfg.cpu_percent {
        out.push(Alert { kind: AlertKind::Cpu, message: format!("CPU at {:.0}% (≥ {:.0}%)", snap.cpu.total, cfg.cpu_percent) });
    }
    if snap.memory.total > 0 {
        let pct = snap.memory.used as f64 / snap.memory.total as f64 * 100.0;
        if pct >= cfg.memory_percent {
            out.push(Alert { kind: AlertKind::Memory, message: format!("Memory at {pct:.0}% (≥ {:.0}%)", cfg.memory_percent) });
        }
    }
    for d in &snap.disks {
        if d.vol_total > 0 {
            let free_pct = d.vol_free as f64 / d.vol_total as f64 * 100.0;
            if free_pct <= cfg.disk_free_percent {
                out.push(Alert {
                    kind: AlertKind::DiskFull,
                    message: format!("{} nearly full: {:.1}% free (≤ {:.0}%)", d.name, free_pct, cfg.disk_free_percent),
                });
            }
        }
    }
    if let Some(t) = sensors.cpu_temp_c {
        if t >= cfg.temp_c {
            out.push(Alert { kind: AlertKind::Temperature, message: format!("CPU temperature {t:.0}°C (≥ {:.0}°C)", cfg.temp_c) });
        }
    }
    if battery.present && !battery.charging && !battery.on_ac && battery.percent <= cfg.battery_percent {
        out.push(Alert { kind: AlertKind::BatteryLow, message: format!("Battery at {:.0}% (≤ {:.0}%)", battery.percent, cfg.battery_percent) });
    }
    for name in &cfg.watch_processes {
        if !snap.processes.iter().any(|p| !p.is_dead && p.name.eq_ignore_ascii_case(name)) {
            out.push(Alert { kind: AlertKind::ProcessGone, message: format!("'{name}' is not running") });
        }
    }
    out
}

/// Raise-once semantics: a condition alerts when it *appears*, then stays
/// quiet until it clears and re-appears (like SI's notification model).
#[derive(Default)]
pub struct AlertTracker {
    active: Vec<String>,
    raised: u32,
}

impl AlertTracker {
    /// Returns the messages of *newly raised* alerts this tick.
    pub fn update(&mut self, current: &[Alert]) -> Vec<String> {
        let now: Vec<String> = current.iter().map(|a| a.message.clone()).collect();
        let new: Vec<String> = now.iter().filter(|m| !self.active.contains(m)).cloned().collect();
        self.raised += new.len() as u32;
        self.active = now;
        new
    }
    pub fn raised_count(&self) -> usize {
        self.raised as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn snap_with(cpu: f32, mem_used: u64, mem_total: u64) -> Snapshot {
        let mut s = Snapshot::default();
        s.cpu.total = cpu;
        s.memory.used = mem_used;
        s.memory.total = mem_total;
        s
    }

    #[test]
    fn cpu_and_memory_rules() {
        let cfg = AlertConfig::default();
        let s = snap_with(95.0, 15, 16);
        let alerts = evaluate(&cfg, &s, &SensorInfo::default(), &BatteryInfo::default());
        assert!(alerts.iter().any(|a| a.kind == AlertKind::Cpu));
        assert!(alerts.iter().any(|a| a.kind == AlertKind::Memory));
        let ok = snap_with(10.0, 4, 16);
        assert!(evaluate(&cfg, &ok, &SensorInfo::default(), &BatteryInfo::default()).is_empty());
    }

    #[test]
    fn disabled_means_silent() {
        let cfg = AlertConfig { enabled: false, ..Default::default() };
        let s = snap_with(99.0, 15, 16);
        assert!(evaluate(&cfg, &s, &SensorInfo::default(), &BatteryInfo::default()).is_empty());
    }

    #[test]
    fn battery_only_when_discharging() {
        let cfg = AlertConfig::default();
        let b = BatteryInfo { present: true, charging: true, percent: 5.0, ..Default::default() };
        assert!(evaluate(&cfg, &Snapshot::default(), &SensorInfo::default(), &b).is_empty());
        let b2 = BatteryInfo { present: true, charging: false, on_ac: false, percent: 5.0, ..Default::default() };
        assert!(evaluate(&cfg, &Snapshot::default(), &SensorInfo::default(), &b2).iter().any(|a| a.kind == AlertKind::BatteryLow));
    }

    #[test]
    fn watched_process_gone() {
        let cfg = AlertConfig { watch_processes: vec!["Safari".into()], ..Default::default() };
        let mut s = Snapshot::default();
        s.processes.push(ProcessInfo { name: "Finder".into(), ..Default::default() });
        assert!(evaluate(&cfg, &s, &SensorInfo::default(), &BatteryInfo::default()).iter().any(|a| a.kind == AlertKind::ProcessGone));
        s.processes.push(ProcessInfo { name: "Safari".into(), ..Default::default() });
        assert!(!evaluate(&cfg, &s, &SensorInfo::default(), &BatteryInfo::default()).iter().any(|a| a.kind == AlertKind::ProcessGone));
    }

    #[test]
    fn tracker_raises_once_until_cleared() {
        let mut t = AlertTracker::default();
        let a = vec![Alert { kind: AlertKind::Cpu, message: "CPU at 95%".into() }];
        assert_eq!(t.update(&a).len(), 1); // raises
        assert!(t.update(&a).is_empty()); // still hot — quiet
        assert!(t.update(&[]).is_empty()); // cleared — quiet
        assert_eq!(t.update(&a).len(), 1); // re-raises
    }
}
