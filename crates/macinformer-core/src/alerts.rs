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
            battery_percent: 10.0,
            watch_processes: Vec::new(),
        }
    }
}

/// Evaluate all rules against one snapshot. Pure — no I/O.
pub fn evaluate(
    cfg: &AlertConfig,
    snap: &Snapshot,
    sensors: &SensorInfo,
    battery: &BatteryInfo,
) -> Vec<Alert> {
    let mut out = Vec::new();
    if !cfg.enabled {
        return out;
    }
    if snap.cpu.total as f64 >= cfg.cpu_percent {
        out.push(Alert {
            kind: AlertKind::Cpu,
            message: format!("CPU usage {:.0}% ≥ {:.0}%", snap.cpu.total, cfg.cpu_percent),
        });
    }
    if snap.memory.total > 0 {
        let pct = snap.memory.used as f64 / snap.memory.total as f64 * 100.0;
        if pct >= cfg.memory_percent {
            out.push(Alert {
                kind: AlertKind::Memory,
                message: format!("Memory {pct:.0}% used ≥ {:.0}%", cfg.memory_percent),
            });
        }
    }
    for d in &snap.disks {
        if d.vol_total > 0 {
            let free_pct = d.vol_free as f64 / d.vol_total as f64 * 100.0;
            if free_pct <= cfg.disk_free_percent {
                out.push(Alert {
                    kind: AlertKind::DiskFull,
                    message: format!(
                        "Disk {} ({}) {:.1}% free ≤ {:.0}%",
                        d.name, d.mount, free_pct, cfg.disk_free_percent
                    ),
                });
            }
        }
    }
    for (label, t) in [
        ("CPU", sensors.soc_max_c.or(sensors.cpu_temp_c)),
        ("GPU", sensors.gpu_temp_c),
        ("SSD", sensors.ssd_temp_c),
    ] {
        if let Some(t) = t {
            if t >= cfg.temp_c {
                out.push(Alert {
                    kind: AlertKind::Temperature,
                    message: format!("{label} temperature {t:.0}°C ≥ {:.0}°C", cfg.temp_c),
                });
            }
        }
    }
    for name in &cfg.watch_processes {
        if !name.is_empty()
            && !snap
                .processes
                .iter()
                .any(|p| !p.is_dead && p.name.contains(name.as_str()))
        {
            out.push(Alert {
                kind: AlertKind::ProcessGone,
                message: format!("Process '{name}' is not running"),
            });
        }
    }
    if battery.present
        && !battery.on_ac
        && !battery.charging
        && battery.percent <= cfg.battery_percent
    {
        out.push(Alert {
            kind: AlertKind::BatteryLow,
            message: format!(
                "Battery {:.0}% ≤ {:.0}% (not charging)",
                battery.percent, cfg.battery_percent
            ),
        });
    }
    out
}

/// Tracks which alerts are currently raised so each fires *once* when it
/// starts (and clears when the condition goes away) — no notification spam.
#[derive(Default)]
pub struct AlertTracker {
    raised: std::collections::HashSet<String>,
}

impl AlertTracker {
    /// Feed current alerts; returns messages that are newly raised this tick.
    pub fn update(&mut self, current: &[Alert]) -> Vec<String> {
        let now: std::collections::HashSet<String> =
            current.iter().map(|a| a.message.clone()).collect();
        let mut fresh: Vec<String> = now
            .iter()
            .filter(|m| !self.raised.contains(*m))
            .cloned()
            .collect();
        fresh.sort();
        self.raised = now;
        fresh
    }

    pub fn raised_count(&self) -> usize {
        self.raised.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn snap() -> Snapshot {
        Snapshot {
            cpu: CpuInfo {
                total: 95.0,
                ..Default::default()
            },
            memory: MemoryInfo {
                total: 100,
                used: 95,
                ..Default::default()
            },
            disks: vec![DiskInfo {
                name: "disk3".into(),
                mount: "/".into(),
                vol_total: 100,
                vol_free: 4,
                ..Default::default()
            }],
            processes: vec![ProcessInfo {
                name: "helperd".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn fires_each_rule() {
        let cfg = AlertConfig {
            watch_processes: vec!["missingd".into()],
            ..Default::default()
        };
        let sens = SensorInfo {
            cpu_temp_c: Some(99.0),
            ..Default::default()
        };
        let bat = BatteryInfo {
            present: true,
            percent: 8.0,
            ..Default::default()
        };
        let alerts = evaluate(&cfg, &snap(), &sens, &bat);
        let kinds: Vec<AlertKind> = alerts.iter().map(|a| a.kind).collect();
        for k in [
            AlertKind::Cpu,
            AlertKind::Memory,
            AlertKind::DiskFull,
            AlertKind::Temperature,
            AlertKind::ProcessGone,
            AlertKind::BatteryLow,
        ] {
            assert!(kinds.contains(&k), "missing {k:?} in {alerts:?}");
        }
    }

    #[test]
    fn quiet_when_healthy() {
        let mut s = snap();
        s.cpu.total = 10.0;
        s.memory.used = 10;
        s.disks[0].vol_free = 60;
        let cfg = AlertConfig::default();
        let alerts = evaluate(&cfg, &s, &SensorInfo::default(), &BatteryInfo::default());
        assert!(alerts.is_empty(), "{alerts:?}");
    }

    #[test]
    fn disabled_is_silent() {
        let cfg = AlertConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(evaluate(
            &cfg,
            &snap(),
            &SensorInfo {
                cpu_temp_c: Some(200.0),
                ..Default::default()
            },
            &BatteryInfo::default()
        )
        .is_empty());
    }

    #[test]
    fn watched_process_present_is_quiet() {
        let cfg = AlertConfig {
            watch_processes: vec!["helper".into()],
            ..Default::default()
        };
        let alerts = evaluate(
            &cfg,
            &Snapshot {
                processes: vec![ProcessInfo {
                    name: "helperd".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            &SensorInfo::default(),
            &BatteryInfo::default(),
        );
        assert!(!alerts.iter().any(|a| a.kind == AlertKind::ProcessGone));
    }

    #[test]
    fn tracker_fires_once_then_clears() {
        let mut t = AlertTracker::default();
        let a = vec![Alert {
            kind: AlertKind::Cpu,
            message: "CPU hot".into(),
        }];
        assert_eq!(t.update(&a), vec!["CPU hot".to_string()]);
        assert!(t.update(&a).is_empty()); // still raised → no repeat
        assert!(t.update(&[]).is_empty()); // cleared
        assert_eq!(t.update(&a), vec!["CPU hot".to_string()]); // re-raised
    }
}
