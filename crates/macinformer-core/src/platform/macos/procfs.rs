//! Process enumeration and control via libproc + sysctl (the same interfaces
//! Activity Monitor and `ps` use), plus per-process rusage for I/O rates.

use super::ffi::*;
use crate::types::*;
use core::ffi::c_int;
use std::collections::HashMap;

pub struct ProcPrev {
    pub total_ns: u64,
    pub io_read: u64,
    pub io_write: u64,
}

fn bsdinfo(pid: i32) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; BSDINFO_SIZE + 16];
    let r = unsafe { proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
    (r as usize >= BSDINFO_SIZE).then_some(buf)
}

fn taskinfo(pid: i32) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; TASKINFO_SIZE + 16];
    let r = unsafe { proc_pidinfo(pid, PROC_PIDTASKINFO, 0, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
    (r as usize >= TASKINFO_SIZE).then_some(buf)
}

fn vnodepath(pid: i32) -> String {
    let mut buf = vec![0u8; VNODEPATHINFO_SIZE + 16];
    let r = unsafe { proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, 0, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
    if r as usize >= VNODEPATH_EXE_OFFSET + 2 {
        r_cstr(&buf, VNODEPATH_EXE_OFFSET)
    } else {
        String::new()
    }
}

fn rusage4(pid: i32) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; RUSAGE_V4_SIZE];
    let r = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V4, buf.as_mut_ptr() as *mut _) };
    (r == 0).then_some(buf)
}

fn state_char(status: u32) -> char {
    // XNU pbi_status: 1=SIDL 2=SRUN 3=SSLEEP 4=SSTOP 5=SZOMB
    match status {
        2 => 'R',
        3 => 'S',
        4 => 'T',
        5 => 'Z',
        _ => 'U',
    }
}

fn list_pids() -> Vec<i32> {
    let mut n = unsafe { proc_listpids(PROC_ALL_PIDS, 0, core::ptr::null_mut(), 0) };
    if n <= 0 {
        return Vec::new();
    }
    n += 64; // races
    let mut buf = vec![0i32; (n as usize) / 4 + 64];
    let got = unsafe { proc_listpids(PROC_ALL_PIDS, 0, buf.as_mut_ptr() as *mut _, (buf.len() * 4) as i32) };
    if got <= 0 {
        return Vec::new();
    }
    buf.truncate(got as usize / 4);
    buf.retain(|&p| p > 0);
    buf
}

/// Enumerate all processes. `prev`/`dt_ns` drive CPU% and I/O rate deltas.
pub fn enumerate(
    prev: &mut HashMap<i32, ProcPrev>,
    dt_ns: u64,
    ncpu: u64,
    mark_new: bool,
) -> (Vec<ProcessInfo>, HashMap<i32, ProcPrev>) {
    let mut out = Vec::new();
    let mut next: HashMap<i32, ProcPrev> = HashMap::with_capacity(512);
    for pid in list_pids() {
        let Some(b) = bsdinfo(pid) else { continue };
        let t = taskinfo(pid);
        let ru = rusage4(pid);

        let total_ns = t.as_ref().map(|t| r_u64(t, PTI_TOTAL_USER) + r_u64(t, PTI_TOTAL_SYSTEM)).unwrap_or(0);
        let io_r = ru.as_ref().map(|r| r_u64(r, RI_DISKIO_READ)).unwrap_or(0);
        let io_w = ru.as_ref().map(|r| r_u64(r, RI_DISKIO_WRITTEN)).unwrap_or(0);

        let (cpu, dr, dw) = match prev.get(&pid) {
            Some(p) if dt_ns > 0 => {
                let cpu = total_ns.saturating_sub(p.total_ns) as f64 / dt_ns as f64 / ncpu as f64 * 100.0;
                let secs = dt_ns as f64 / 1e9;
                (cpu as f32, (io_r.saturating_sub(p.io_read)) as f64 / secs, (io_w.saturating_sub(p.io_write)) as f64 / secs)
            }
            _ => (0.0, 0.0, 0.0),
        };
        next.insert(pid, ProcPrev { total_ns, io_read: io_r, io_write: io_w });

        let path = vnodepath(pid);
        let uid = r_u32(&b, PBI_UID);
        let mut name = r_cstr(&b, PBI_NAME);
        if name.is_empty() {
            name = r_cstr(&b, PBI_COMM);
        }
        if !path.is_empty() {
            if let Some(base) = path.rsplit('/').next() {
                if !base.is_empty() {
                    name = base.to_string();
                }
            }
        }
        let memory = ru
            .as_ref()
            .map(|r| r_u64(r, RI_PHYS_FOOTPRINT))
            .filter(|&m| m > 0)
            .or_else(|| t.as_ref().map(|t| r_u64(t, PTI_RESIDENT_SIZE)))
            .unwrap_or(0);

        out.push(ProcessInfo {
            pid: r_i32(&b, PBI_PID),
            ppid: r_i32(&b, PBI_PPID),
            uid,
            user: username_of(uid),
            name,
            is_platform_binary: path.starts_with("/System/") || path.starts_with("/usr/libexec/") || path.starts_with("/bin/") || path.starts_with("/sbin/") || path.starts_with("/usr/bin/") || path.starts_with("/usr/sbin/"),
            path,
            cmdline: cmdline(pid),
            state: state_char(r_u32(&b, PBI_STATUS)),
            cpu_percent: cpu,
            memory_bytes: memory,
            virtual_bytes: t.as_ref().map(|t| r_u64(t, PTI_VIRTUAL_SIZE)).unwrap_or(0),
            threads: t.as_ref().map(|t| r_u32(t, PTI_THREADNUM)).unwrap_or(0),
            open_fds: r_u32(&b, PBI_NFILES),
            io_read_bps: dr,
            io_write_bps: dw,
            start_time: r_u64(&b, PBI_START_TVSEC),
            nice: r_i32(&b, PBI_NICE),
            is_root_owned: uid == 0,
            is_new: mark_new && !prev.contains_key(&pid),
            is_dead: false,
        });
    }
    (out, next)
}

/// Full command line via KERN_PROCARGS2 (best-effort; SIP may deny).
pub fn cmdline(pid: i32) -> String {
    procargs(pid).map(|(_, args)| args.join(" ")).unwrap_or_default()
}

/// Parse kern.procargs2: [argc:i32][exec_path\0][argv0\0..][env\0..]\0
fn procargs(pid: i32) -> Option<(String, Vec<String>)> {
    const CTL_KERN: c_int = 1;
    const KERN_PROCARGS2: c_int = 49;
    unsafe extern "C" {
        fn sysctl(name: *mut c_int, namelen: u32, oldp: *mut core::ffi::c_void, oldlenp: *mut usize, newp: *mut core::ffi::c_void, newlen: usize) -> c_int;
    }
    let mut mib = [CTL_KERN, KERN_PROCARGS2, pid];
    let mut size: usize = 0;
    if unsafe { sysctl(mib.as_mut_ptr(), 3, core::ptr::null_mut(), &mut size, core::ptr::null_mut(), 0) } != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size + 8];
    if unsafe { sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr() as *mut _, &mut size, core::ptr::null_mut(), 0) } != 0 {
        return None;
    }
    let argc = r_i32(&buf, 0).max(0) as usize;
    let mut pos = 4;
    let exec = r_cstr(&buf, pos);
    pos += exec.len() + 1;
    // skip NUL padding + argv strings
    let mut args = Vec::new();
    while pos < buf.len() && buf[pos] == 0 {
        pos += 1;
    }
    for _ in 0..argc {
        if pos >= buf.len() {
            break;
        }
        let s = r_cstr(&buf, pos);
        pos += s.len() + 1;
        args.push(s);
    }
    Some((exec, args))
}

/// Environment variables (Properties > Environment page).
pub fn environment(pid: i32) -> Vec<(String, String)> {
    const CTL_KERN: c_int = 1;
    const KERN_PROCARGS2: c_int = 49;
    unsafe extern "C" {
        fn sysctl(name: *mut c_int, namelen: u32, oldp: *mut core::ffi::c_void, oldlenp: *mut usize, newp: *mut core::ffi::c_void, newlen: usize) -> c_int;
    }
    let mut mib = [CTL_KERN, KERN_PROCARGS2, pid];
    let mut size: usize = 0;
    if unsafe { sysctl(mib.as_mut_ptr(), 3, core::ptr::null_mut(), &mut size, core::ptr::null_mut(), 0) } != 0 || size == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; size + 8];
    if unsafe { sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr() as *mut _, &mut size, core::ptr::null_mut(), 0) } != 0 {
        return Vec::new();
    }
    let argc = r_i32(&buf, 0).max(0) as usize;
    let mut pos = 4;
    let exec = r_cstr(&buf, pos);
    pos += exec.len() + 1;
    for _ in 0..=argc {
        // skip argv[0..argc-1] plus padding/exe repeats
        if pos >= buf.len() {
            return Vec::new();
        }
        let s = r_cstr(&buf, pos);
        pos += s.len() + 1;
    }
    let mut env = Vec::new();
    while pos < buf.len() {
        let s = r_cstr(&buf, pos);
        if s.is_empty() {
            break;
        }
        pos += s.len() + 1;
        if let Some(eq) = s.find('=') {
            env.push((s[..eq].to_string(), s[eq + 1..].to_string()));
        }
    }
    env
}

// ------------------------------------------------------------- actions

pub fn send_signal(pid: i32, sig: i32) -> Result<(), String> {
    let r = unsafe { kill(pid, sig) };
    if r == 0 {
        Ok(())
    } else {
        Err(format!("kill({pid}, {sig}) failed: {}", std::io::Error::last_os_error()))
    }
}

pub fn set_nice(pid: i32, nice: i32) -> Result<(), String> {
    let r = unsafe { setpriority(PRIO_PROCESS, pid, nice.clamp(-20, 20)) };
    if r == 0 {
        Ok(())
    } else {
        Err(format!("setpriority({pid}, {nice}) failed: {} (need root to lower nice)", std::io::Error::last_os_error()))
    }
}

/// codesign details for the Properties view (cached by caller).
pub fn signing_info(path: &str) -> (String, String) {
    let out = std::process::Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4", path])
        .output();
    let Ok(o) = out else { return (String::new(), String::new()) };
    let text = String::from_utf8_lossy(&o.stderr);
    let mut authority = String::new();
    let mut flags = String::new();
    for line in text.lines() {
        if let Some(a) = line.strip_prefix("Authority=") {
            if authority.is_empty() {
                authority = a.trim().to_string();
            }
        } else if let Some(f) = line.strip_prefix("CodeDirectory v=") {
            let _ = f;
        } else if line.starts_with("flags=") {
            flags = line.trim().to_string();
        }
    }
    (authority, flags)
}
