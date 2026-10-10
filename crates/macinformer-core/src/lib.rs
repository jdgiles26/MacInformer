//! macinformer-core — data model and OS collectors.
//!
//! Zero external dependencies: every metric is read through direct OS FFI
//! (libproc / Mach / IOKit / sysctl on macOS; /proc on the Linux fallback),
//! mirroring System Informer's own dependency-light design.

pub mod alerts;
pub mod bandwidth;
pub mod history;
pub mod json;
pub mod llm;
pub mod platform;
pub mod recorder;
pub mod report;
pub mod resolver;
pub mod search;
pub mod sensors;
pub mod server;
pub mod sysrec;
pub mod types;

pub use alerts::{evaluate as evaluate_alerts, Alert, AlertConfig, AlertKind, AlertTracker};
pub use bandwidth::{BandwidthSampler, ProcBandwidth};
pub use history::HistoryRing;
pub use recorder::{hms_of, ConnEvent, ConnEventKind, NetRecorder, ProcNetStats};
pub use search::Matcher;
pub use sensors::{battery as read_battery, sensors as read_sensors, BatteryInfo, SensorInfo};
pub use sysrec::{SysEvent, SysEventKind, SysRecorder};
pub use types::*;

/// A collector produces point-in-time snapshots and on-demand detail.
/// It is stateful because rates (CPU%, bytes/s) are computed from deltas.
pub trait Collector {
    /// Full system snapshot: processes + CPU + memory + disks + NICs + GPU.
    fn snapshot(&mut self) -> Snapshot;
    /// All TCP/UDP connections with owning process (Network tab).
    fn connections(&mut self) -> Vec<Connection>;
    /// All open file descriptors system-wide (Find Handles or DLLs equivalent).
    fn open_files(&mut self) -> Vec<OpenFile>;
    /// launchd services (Services tab).
    fn services(&mut self) -> Vec<ServiceInfo>;
    /// Environment variables of a process (Properties > Environment).
    fn environment(&self, pid: i32) -> Vec<(String, String)>;
    /// Extended per-process detail for the Properties view.
    fn process_detail(&mut self, pid: i32) -> Option<ProcessDetail>;

    // ---- actions (process context menu) ----
    fn send_signal(&self, pid: i32, sig: i32) -> Result<(), String>;
    fn set_nice(&self, pid: i32, nice: i32) -> Result<(), String>;
    fn service_action(&self, label: &str, action: ServiceAction) -> Result<String, String>;

    /// Human-readable capability notes (privilege/SIP limitations) for the UI.
    fn capability_notes(&self) -> Vec<String>;
    /// True when running with root privileges.
    fn is_root(&self) -> bool;

    /// Per-application network bandwidth (rx/tx rates). Populated on macOS
    /// via the background nettop sampler; empty where no supported per-process
    /// counter source exists. Never blocks the caller.
    fn proc_bandwidth(&mut self) -> (Vec<ProcBandwidth>, String) {
        (Vec::new(), "per-app rates unsupported on this platform".into())
    }
}

/// Signals used by the process context menu (mirrors SI Terminate/Suspend/...).
pub mod sig {
    pub const TERM: i32 = 15;
    pub const KILL: i32 = 9;
    pub const STOP: i32 = 17;
    pub const CONT: i32 = 19;
    pub const HUP: i32 = 1;
}

/// Construct the collector for the current operating system.
/// `Send` so `--server` can drive it from its producer thread.
pub fn system_collector() -> Box<dyn Collector + Send> {
    platform::new_collector()
}
