//! Shared data model — the macOS-native equivalent of System Informer's
//! PH_PROCESS_ITEM / PH_NETWORK_ITEM / PH_SERVICE_ITEM / PH_DISK_ITEM records.

/// One row of the Processes tab.
#[derive(Clone, Debug, Default)]
pub struct ProcessInfo {
    pub pid: i32,
    pub ppid: i32,
    pub uid: u32,
    pub user: String,
    pub name: String,
    /// Full path of the executable ("" if unreadable).
    pub path: String,
    /// First line of the command line.
    pub cmdline: String,
    /// Single-letter state (R/S/Z/U...) like Activity Monitor's `ps`.
    pub state: char,
    pub cpu_percent: f32,
    /// Physical footprint in bytes when available, else RSS.
    pub memory_bytes: u64,
    pub virtual_bytes: u64,
    pub threads: u32,
    pub open_fds: u32,
    pub io_read_bps: f64,
    pub io_write_bps: f64,
    /// Unix start time.
    pub start_time: u64,
    pub nice: i32,
    /// uid == 0 (SI "system process" highlighting).
    pub is_root_owned: bool,
    /// Lives under /System or /usr/libexec (SIP platform binary → restricted).
    pub is_platform_binary: bool,
    /// Marks a process seen for the first time (SI green highlight).
    pub is_new: bool,
    /// Marks a process that exited since last refresh (SI red highlight).
    pub is_dead: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CpuInfo {
    /// 0..100 per logical core.
    pub per_core: Vec<f32>,
    /// 0..100 aggregate.
    pub total: f32,
    pub brand: String,
    pub logical_count: u32,
    pub physical_count: u32,
    /// Apple Silicon performance-core count (0 on Intel).
    pub perf_cores: u32,
    /// Apple Silicon efficiency-core count.
    pub eff_cores: u32,
    /// CPU frequency in Hz when the OS exposes it (None on Apple Silicon).
    pub freq_hz: Option<u64>,
    pub load_avg: [f64; 3],
    /// Delta since previous snapshot.
    pub ctx_switches_delta: u64,
    pub syscalls_delta: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MemoryInfo {
    pub total: u64,
    pub free: u64,
    pub active: u64,
    pub inactive: u64,
    pub wired: u64,
    pub compressed: u64,
    pub purgeable: u64,
    pub speculative: u64,
    /// app memory + wired + compressed (Activity Monitor "Memory Used").
    pub used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub pageins_delta: u64,
    pub pageouts_delta: u64,
}

/// One physical drive (Disk tab + per-disk graphs).
#[derive(Clone, Debug, Default)]
pub struct DiskInfo {
    /// BSD name, e.g. "disk0".
    pub name: String,
    pub model: String,
    pub size_bytes: u64,
    pub read_bps: f64,
    pub write_bps: f64,
    pub total_read: u64,
    pub total_written: u64,
    /// Primary mount of this drive ("" if none).
    pub mount: String,
    pub fs_type: String,
    pub vol_total: u64,
    pub vol_free: u64,
}

/// One network interface (Ethernet/Wi-Fi graphs).
#[derive(Clone, Debug, Default)]
pub struct NetIfInfo {
    pub name: String,
    pub display_name: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub total_rx: u64,
    pub total_tx: u64,
    pub is_up: bool,
    pub mtu: u32,
}

/// Apple GPU statistics from IOKit IOAccelerator.
#[derive(Clone, Debug, Default)]
pub struct GpuInfo {
    pub name: String,
    pub utilization: f32,
    /// "In use system memory" — unified memory held by the GPU.
    pub memory_used: u64,
    pub memory_allocated: u64,
    /// Requires root (powermetrics).
    pub power_watts: Option<f64>,
    pub temperature_c: Option<f64>,
}

/// One row of the Network tab (PH_NETWORK_ITEM equivalent).
#[derive(Clone, Debug)]
pub struct Connection {
    pub pid: i32,
    pub process_name: String,
    pub protocol: Protocol,
    pub local_addr: String,
    pub local_port: u16,
    pub remote_addr: String,
    pub remote_port: u16,
    pub state: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp4,
    Tcp6,
    Udp4,
    Udp6,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Tcp4 => "TCP",
            Protocol::Tcp6 => "TCP6",
            Protocol::Udp4 => "UDP",
            Protocol::Udp6 => "UDP6",
        }
    }
}

/// One row of the Find Open Files view (Ctrl+F / lsof equivalent).
#[derive(Clone, Debug)]
pub struct OpenFile {
    pub pid: i32,
    pub process_name: String,
    pub fd: i32,
    pub kind: FdKind,
    /// Path for files, "local -> remote" for sockets, type name otherwise.
    pub target: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdKind {
    File,
    Socket,
    Pipe,
    Kqueue,
    PsxShm,
    PsxSem,
    Fsevents,
    Guarded,
    Other,
}

impl FdKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FdKind::File => "File",
            FdKind::Socket => "Socket",
            FdKind::Pipe => "Pipe",
            FdKind::Kqueue => "Kqueue",
            FdKind::PsxShm => "PSXSHM",
            FdKind::PsxSem => "PSXSEM",
            FdKind::Fsevents => "FSEvents",
            FdKind::Guarded => "Guarded",
            FdKind::Other => "Other",
        }
    }
}

/// One row of the Services tab (launchd job).
#[derive(Clone, Debug, Default)]
pub struct ServiceInfo {
    pub label: String,
    /// 0 = not running.
    pub pid: i32,
    pub last_exit_status: i64,
    pub program: String,
    pub run_at_load: bool,
    pub keep_alive: bool,
    pub disabled: bool,
    /// "system", "user", "gui/<uid>", or "plist" (defined but not loaded).
    pub domain: String,
    pub plist_path: String,
}

impl ServiceInfo {
    pub fn status(&self) -> String {
        if self.pid > 0 {
            format!("Running ({})", self.pid)
        } else if self.disabled {
            "Disabled".into()
        } else {
            "Stopped".into()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Kickstart,
    Enable,
    Disable,
}

/// Extended Properties view data for one process.
#[derive(Clone, Debug, Default)]
pub struct ProcessDetail {
    pub info: ProcessInfo,
    pub environment: Vec<(String, String)>,
    pub open_files: Vec<OpenFile>,
    pub signing_authority: String,
    pub signing_flags: String,
    /// Why deeper introspection (threads/stacks/memory) is unavailable, if so.
    pub introspection_note: String,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub processes: Vec<ProcessInfo>,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetIfInfo>,
    pub gpu: Option<GpuInfo>,
    pub uptime_secs: u64,
    pub process_total: u32,
    pub thread_total: u32,
    pub fd_total: u64,
}

// ---------- formatting helpers (SI-style IEC units) ----------

pub fn human_bytes(v: u64) -> String {
    const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
    if v < 1024 {
        return format!("{v} B");
    }
    let mut f = v as f64;
    let mut i = 0;
    while f >= 1024.0 && i < UNITS.len() - 1 {
        f /= 1024.0;
        i += 1;
    }
    if f >= 100.0 {
        format!("{f:.0} {}", UNITS[i])
    } else if f >= 10.0 {
        format!("{f:.1} {}", UNITS[i])
    } else {
        format!("{f:.2} {}", UNITS[i])
    }
}

pub fn human_rate(bps: f64) -> String {
    if bps < 1.0 {
        return "0".into();
    }
    format!("{}/s", human_bytes(bps as u64))
}

pub fn human_duration(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

pub fn current_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

pub fn username_of(uid: u32) -> String {
    #[repr(C)]
    struct Passwd {
        pw_name: *const u8,
        pw_passwd: *const u8,
        pw_uid: u32,
        pw_gid: u32,
        pw_gecos: *const u8,
        pw_dir: *const u8,
        pw_shell: *const u8,
    }
    unsafe extern "C" {
        fn getpwuid(uid: u32) -> *mut Passwd;
    }
    unsafe {
        let p = getpwuid(uid);
        if p.is_null() || (*p).pw_name.is_null() {
            return uid.to_string();
        }
        let mut s = Vec::new();
        let mut q = (*p).pw_name;
        while *q != 0 {
            s.push(*q);
            q = q.add(1);
        }
        String::from_utf8_lossy(&s).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_fmt() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.00 kB");
        assert_eq!(human_bytes(1536), "1.50 kB");
        assert_eq!(human_bytes(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn duration_fmt() {
        assert_eq!(human_duration(3661), "01:01:01");
        assert_eq!(human_duration(90061), "1d 01:01:01");
    }
}
