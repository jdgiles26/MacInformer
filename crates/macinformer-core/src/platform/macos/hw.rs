//! CPU / memory / GPU / thermal collectors (Mach host APIs + IOKit).

use super::ffi::*;
use crate::types::*;

// ------------------------------------------------------------------ CPU

/// Read per-core tick counters: Vec<[user, system, idle, nice]>.
pub fn cpu_ticks() -> Vec<[u64; 4]> {
    unsafe {
        let host = mach_host_self();
        let mut count: u32 = 0;
        let mut info: *mut i32 = core::ptr::null_mut();
        let mut info_count: u32 = 0;
        let kr = host_processor_info(host, PROCESSOR_CPU_LOAD_INFO, &mut count, &mut info, &mut info_count);
        if kr != 0 || info.is_null() {
            return Vec::new();
        }
        let n = count as usize;
        let slice = core::slice::from_raw_parts(info as *const u32, (info_count as usize).min(n * 4));
        let mut out = Vec::with_capacity(n);
        for c in 0..n {
            let b = c * 4;
            if b + 3 < slice.len() + 1 && b + 4 <= slice.len() {
                out.push([
                    slice[b] as u64,
                    slice[b + 1] as u64,
                    slice[b + 2] as u64,
                    slice[b + 3] as u64,
                ]);
            }
        }
        mach_vm_deallocate(mach_task_self_, info as u64, (info_count as u64) * 4);
        out
    }
}

/// Compute per-core busy% between two tick samples.
pub fn cpu_usage(prev: &[[u64; 4]], cur: &[[u64; 4]]) -> Vec<f32> {
    cur.iter()
        .zip(prev.iter())
        .map(|(c, p)| {
            let user = c[0].saturating_sub(p[0]) + c[3].saturating_sub(p[3]);
            let sys = c[1].saturating_sub(p[1]);
            let idle = c[2].saturating_sub(p[2]);
            let total = user + sys + idle;
            if total == 0 {
                0.0
            } else {
                ((user + sys) as f64 / total as f64 * 100.0) as f32
            }
        })
        .collect()
}

pub fn cpu_static_info() -> (String, u32, u32, u32, u32) {
    let logical = sysctl_u64("hw.ncpu").unwrap_or(1) as u32;
    let physical = sysctl_u64("hw.physicalcpu").unwrap_or(logical as u64) as u32;
    let perf = sysctl_u64("hw.perflevel0.physicalcpu").unwrap_or(0) as u32;
    let eff = sysctl_u64("hw.perflevel1.physicalcpu").unwrap_or(0) as u32;
    let brand = sysctl_string("machdep.cpu.brand_string")
        .or_else(|| {
            // Apple Silicon: compose from hw.model
            let model = sysctl_string("hw.model").unwrap_or_else(|| "Apple Silicon".into());
            let p = if perf > 0 { format!("{perf}P+{eff}E") } else { format!("{physical}-core") };
            Some(format!("Apple Silicon ({model}, {p})"))
        })
        .unwrap_or_else(|| "Unknown CPU".into());
    (brand, logical, physical, perf, eff)
}

pub fn load_avg() -> [f64; 3] {
    let mut la = [0.0f64; 3];
    unsafe { getloadavg(la.as_mut_ptr(), 3) };
    la
}

// ------------------------------------------------------------------ Memory

pub fn memory_info(page_size: u64, prev: Option<(u64, u64)>) -> MemoryInfo {
    let mut info = MemoryInfo::default();
    let mut buf = [0i32; 64];
    let mut count: u32 = 64;
    let kr = unsafe { host_statistics64(mach_host_self(), HOST_VM_INFO64, buf.as_mut_ptr(), &mut count) };
    if kr != 0 {
        return info;
    }
    let raw = unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };
    let p = |off: usize| r_u32(raw, off) as u64 * page_size;
    info.free = p(VM64_FREE);
    info.active = p(VM64_ACTIVE);
    info.inactive = p(VM64_INACTIVE);
    info.wired = p(VM64_WIRE);
    info.compressed = p(VM64_COMPRESSOR);
    info.purgeable = p(VM64_PURGEABLE);
    info.speculative = p(VM64_SPECULATIVE);
    let internal = p(VM64_INTERNAL);
    info.total = sysctl_u64("hw.memsize").unwrap_or(0);
    // Activity Monitor "Memory Used" = app memory (internal - purgeable) + wired + compressed
    info.used = internal.saturating_sub(info.purgeable) + info.wired + info.compressed;
    let pageins = r_u64(raw, VM64_PAGEINS);
    let pageouts = r_u64(raw, VM64_PAGEOUTS);
    if let Some((pi, po)) = prev {
        info.pageins_delta = pageins.saturating_sub(pi);
        info.pageouts_delta = pageouts.saturating_sub(po);
    }
    // vm.swapusage: struct xsw_usage { u64 total, avail, used; i32 pagesize; bool enc }
    unsafe {
        let mut xsw = [0u8; 40];
        let mut len = xsw.len();
        let name = std::ffi::CString::new("vm.swapusage").unwrap();
        if sysctlbyname(name.as_ptr(), xsw.as_mut_ptr() as *mut _, &mut len, core::ptr::null_mut(), 0) == 0 {
            info.swap_total = r_u64(&xsw, 0);
            info.swap_used = r_u64(&xsw, 16);
        }
    }
    info
}

pub fn page_counters() -> (u64, u64) {
    let mut buf = [0i32; 64];
    let mut count: u32 = 64;
    let kr = unsafe { host_statistics64(mach_host_self(), HOST_VM_INFO64, buf.as_mut_ptr(), &mut count) };
    if kr != 0 {
        return (0, 0);
    }
    let raw = unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };
    (r_u64(raw, VM64_PAGEINS), r_u64(raw, VM64_PAGEOUTS))
}

pub fn page_size() -> u64 {
    sysctl_u64("hw.pagesize").unwrap_or(16384)
}

pub fn uptime_secs() -> u64 {
    // kern.boottime: struct timeval { i64 tv_sec; i32 tv_usec; pad }
    unsafe {
        let mut tv = [0u8; 16];
        let mut len = tv.len();
        let name = std::ffi::CString::new("kern.boottime").unwrap();
        if sysctlbyname(name.as_ptr(), tv.as_mut_ptr() as *mut _, &mut len, core::ptr::null_mut(), 0) == 0 {
            let boot = r_u64(&tv, 0);
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            return now.saturating_sub(boot);
        }
    }
    0
}

// ------------------------------------------------------------------ GPU

/// Apple GPU via IOKit IOAccelerator PerformanceStatistics
/// (works on AGX / Apple Silicon; the same source Activity Monitor uses).
pub fn gpu_info() -> Option<GpuInfo> {
    let mut gpu = GpuInfo::default();
    let mut found = false;
    with_ioservices("IOAccelerator", |entry| {
        if found {
            return;
        }
        if let Some(stats) = iokit_prop_dict(entry, "PerformanceStatistics") {
            let d = stats.as_ref();
            let util = cfdict_number(d, "Device Utilization %")
                .or_else(|| cfdict_number(d, "Utilization %"))
                .or_else(|| cfdict_number(d, "GPU Activity(%)"));
            if let Some(u) = util {
                gpu.utilization = u as f32;
                gpu.memory_used = cfdict_number(d, "In use system memory").unwrap_or(0.0) as u64;
                gpu.memory_allocated = cfdict_number(d, "Alloc system memory").unwrap_or(0.0) as u64;
                let mut name = [0i8; 128];
                if unsafe { IORegistryEntryGetName(entry, name.as_mut_ptr()) } == 0 {
                    gpu.name = r_cstr(unsafe { core::slice::from_raw_parts(name.as_ptr() as *const u8, 128) }, 0);
                }
                found = true;
            }
        }
    });
    found.then_some(gpu)
}

// ------------------------------------------------------- thermal / power

/// Apple Silicon die temperatures and GPU power via `powermetrics` (root only).
/// Returns (gpu_watts, gpu_temp_c, cpu_temp_c). Empty when unavailable.
pub fn power_metrics() -> (Option<f64>, Option<f64>, Option<f64>) {
    if (unsafe { geteuid() }) != 0 {
        return (None, None, None);
    }
    let out = std::process::Command::new("/usr/bin/powermetrics")
        .args(["--samplers", "cpu_power,gpu_power,thermal", "-n", "1", "-i", "200", "-o", "text"])
        .output();
    let Ok(o) = out else { return (None, None, None) };
    let text = String::from_utf8_lossy(&o.stdout);
    let mut gpu_w = None;
    let mut gpu_t = None;
    let mut cpu_t = None;
    for line in text.lines() {
        let l = line.trim();
        let grab = |unitless: &str| -> Option<f64> {
            let v: f64 = unitless.split_whitespace().next()?.parse().ok()?;
            Some(v)
        };
        if let Some(rest) = l.strip_prefix("GPU Power:") {
            let mut v = grab(rest).unwrap_or(0.0);
            if rest.contains("mW") {
                v /= 1000.0;
            }
            gpu_w = Some(v);
        } else if l.to_lowercase().starts_with("gpu die temperature") {
            gpu_t = l.split(':').nth(1).and_then(grab);
        } else if l.to_lowercase().starts_with("cpu die temperature") {
            cpu_t = l.split(':').nth(1).and_then(grab);
        }
    }
    (gpu_w, gpu_t, cpu_t)
}
