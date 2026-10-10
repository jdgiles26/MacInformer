//! macOS collector — assembles all subsystem collectors into the
//! `Collector` trait implementation used by the UI.

pub mod fds;
pub mod ffi;
pub mod hw;
pub mod io;
pub mod launchd;
pub mod procfs;

use crate::types::*;
use std::collections::HashMap;
use std::time::Instant;

pub struct MacCollector {
    prev_ticks: Vec<[u64; 4]>,
    prev_proc: HashMap<i32, procfs::ProcPrev>,
    prev_infos: HashMap<i32, ProcessInfo>,
    prev_disks: HashMap<String, (u64, u64)>,
    prev_nets: HashMap<String, (u64, u64)>,
    prev_pages: Option<(u64, u64)>,
    last: Instant,
    first: bool,
    page_size: u64,
    cpu_static: (String, u32, u32, u32, u32),
    refresh_count: u32,
    /// Last powermetrics reading (watts, gpu_temp) — sampled every 5th
    /// refresh, carried over in between instead of being blanked out.
    cached_power: (Option<f64>, Option<f64>),
}

impl Default for MacCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MacCollector {
    pub fn new() -> Self {
        Self {
            prev_ticks: Vec::new(),
            prev_proc: HashMap::new(),
            prev_infos: HashMap::new(),
            prev_disks: HashMap::new(),
            prev_nets: HashMap::new(),
            prev_pages: None,
            last: Instant::now(),
            first: true,
            page_size: hw::page_size(),
            cpu_static: hw::cpu_static_info(),
            refresh_count: 0,
            cached_power: (None, None),
        }
    }
}

impl crate::Collector for MacCollector {
    fn snapshot(&mut self) -> Snapshot {
        let now = Instant::now();
        let dt = now.duration_since(self.last);
        let dt_ns = dt.as_nanos().max(1) as u64;
        let dt_secs = dt.as_secs_f64();
        self.last = now;
        self.refresh_count += 1;

        // CPU
        let ticks = hw::cpu_ticks();
        let per_core = if self.prev_ticks.len() == ticks.len() {
            hw::cpu_usage(&self.prev_ticks, &ticks)
        } else {
            vec![0.0; ticks.len()]
        };
        self.prev_ticks = ticks;
        let total = if per_core.is_empty() { 0.0 } else { per_core.iter().sum::<f32>() / per_core.len() as f32 };
        let (brand, logical, physical, perf, eff) = self.cpu_static.clone();

        // Memory
        let memory = hw::memory_info(self.page_size, self.prev_pages);
        self.prev_pages = Some(hw::page_counters());

        // Processes
        let ncpu = logical.max(1) as u64;
        let (mut processes, next) = procfs::enumerate(&mut self.prev_proc, dt_ns, ncpu, !self.first);
        self.prev_proc = next;
        // dead-process fade (SI red highlight) for one refresh
        let cur: std::collections::HashSet<i32> = processes.iter().map(|p| p.pid).collect();
        if !self.first {
            for (&pid, info) in &self.prev_infos {
                if !cur.contains(&pid) {
                    let mut d = info.clone();
                    d.is_dead = true;
                    d.cpu_percent = 0.0;
                    processes.push(d);
                }
            }
        }
        self.prev_infos = processes.iter().filter(|p| !p.is_dead).map(|p| (p.pid, p.clone())).collect();
        self.first = false;

        // Disks / networks / GPU
        let disks = io::disks(&mut self.prev_disks, dt_secs);
        let networks = io::networks(&mut self.prev_nets, dt_secs);
        let mut gpu = hw::gpu_info();
        if let Some(g) = gpu.as_mut() {
            if self.refresh_count % 5 == 1 {
                let (gpu_w, gpu_t, _cpu_t) = hw::power_metrics();
                self.cached_power = (gpu_w, gpu_t);
            }
            // carry the last successful reading between samples
            g.power_watts = self.cached_power.0;
            g.temperature_c = self.cached_power.1;
        }

        let thread_total = processes.iter().map(|p| p.threads).sum();
        let fd_total = processes.iter().map(|p| p.open_fds as u64).sum();
        Snapshot {
            process_total: processes.iter().filter(|p| !p.is_dead).count() as u32,
            processes,
            cpu: CpuInfo {
                per_core,
                total,
                brand,
                logical_count: logical,
                physical_count: physical,
                perf_cores: perf,
                eff_cores: eff,
                freq_hz: None, // not published on Apple Silicon
                load_avg: hw::load_avg(),
                ctx_switches_delta: 0,
                syscalls_delta: 0,
            },
            memory,
            disks,
            networks,
            gpu,
            uptime_secs: hw::uptime_secs(),
            thread_total,
            fd_total,
        }
    }

    fn connections(&mut self) -> Vec<Connection> {
        fds::connections()
    }

    fn open_files(&mut self) -> Vec<OpenFile> {
        fds::open_files()
    }

    fn services(&mut self) -> Vec<ServiceInfo> {
        launchd::services()
    }

    fn environment(&self, pid: i32) -> Vec<(String, String)> {
        procfs::environment(pid)
    }

    fn process_detail(&mut self, pid: i32) -> Option<ProcessDetail> {
        let mut scratch = HashMap::new();
        let (procs, _) = procfs::enumerate(&mut scratch, 0, 1, false);
        let info = procs.into_iter().find(|p| p.pid == pid)?;
        let environment = procfs::environment(pid);
        let open_files = fds::open_files_of(pid);
        let (authority, flags) = if info.path.is_empty() { (String::new(), String::new()) } else { procfs::signing_info(&info.path) };
        let introspection_note = if info.is_platform_binary {
            "SIP platform binary: thread/stack/memory introspection via task_for_pid is blocked by Apple for this process.".to_string()
        } else if !self.is_root() {
            "Run with sudo for complete FD tables and cross-user introspection.".to_string()
        } else {
            String::new()
        };
        Some(ProcessDetail { info, environment, open_files, signing_authority: authority, signing_flags: flags, introspection_note })
    }

    fn send_signal(&self, pid: i32, sig: i32) -> Result<(), String> {
        procfs::send_signal(pid, sig)
    }

    fn set_nice(&self, pid: i32, nice: i32) -> Result<(), String> {
        procfs::set_nice(pid, nice)
    }

    fn service_action(&self, label: &str, action: ServiceAction) -> Result<String, String> {
        let svcs = launchd::services();
        let svc = svcs.iter().find(|s| s.label == label);
        launchd::service_action(
            label,
            svc.map(|s| s.plist_path.as_str()).unwrap_or(""),
            svc.map(|s| s.domain.as_str()).unwrap_or("user"),
            action,
        )
    }

    fn proc_bandwidth(&mut self) -> (Vec<crate::bandwidth::ProcBandwidth>, String) {
        let s = crate::bandwidth::global_sampler();
        (s.latest(), s.note())
    }

    fn capability_notes(&self) -> Vec<String> {
        let mut v = vec![
            "Process list, CPU/memory, disks, network, GPU: full fidelity (public APIs).".to_string(),
            "Close foreign handle/connection: no public macOS API (same as every macOS tool); terminate owner instead.".to_string(),
            "CPU affinity: macOS has no affinity API (thread affinity tags are hints only).".to_string(),
        ];
        if !self.is_root() {
            v.push("Not root: per-process I/O rates, FD tables and power metrics of other users' processes are limited — run with sudo for full fidelity.".into());
        } else {
            v.push("Root: powermetrics thermal/power sampling enabled.".into());
        }
        v.push("Per-app bandwidth: sampled via Apple's nettop (kernel nstat accounting); rates are approximate, external interfaces only.".into());
        v.push("SIP: Apple platform binaries cannot be attached even as root (by design).".into());
        v
    }

    fn is_root(&self) -> bool {
        (unsafe { ffi::geteuid() }) == 0
    }
}
