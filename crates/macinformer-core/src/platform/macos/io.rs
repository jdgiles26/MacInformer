//! Disk drives (IOKit IOBlockStorageDriver statistics + statfs volumes)
//! and network interfaces (getifaddrs AF_LINK counters).

use super::ffi::*;
use crate::types::*;
use std::collections::HashMap;

// ------------------------------------------------------------------ disks

struct RawDisk {
    name: String,
    model: String,
    size: u64,
    read: u64,
    written: u64,
}

fn raw_disks() -> Vec<RawDisk> {
    let mut out = Vec::new();
    with_ioservices("IOBlockStorageDriver", |entry| {
        let mut name = [0i8; 128];
        if unsafe { IORegistryEntryGetName(entry, name.as_mut_ptr()) } != 0 {
            return;
        }
        let entry_name = r_cstr(unsafe { core::slice::from_raw_parts(name.as_ptr() as *const u8, 128) }, 0);
        let Some(stats) = iokit_prop_dict(entry, "Statistics") else { return };
        let read = cfdict_number(stats.as_ref(), "Bytes (Read)").unwrap_or(0.0) as u64;
        let written = cfdict_number(stats.as_ref(), "Bytes (Written)").unwrap_or(0.0) as u64;
        // Whole-disk BSD name lives on the parent/child media objects; the
        // driver entry name is e.g. "IOBlockStorageDriver". Use the registry
        // name of the entry as fallback and prefer the "BSD Name" property
        // when the driver publishes one (Apple Silicon NVMe does via media).
        let bsd = iokit_prop_string(entry, "BSD Name").unwrap_or_else(|| entry_name.clone());
        let model = iokit_prop_string(entry, "Product Name")
            .or_else(|| iokit_prop_string(entry, "Model"))
            .unwrap_or_else(|| "Apple SSD".into());
        let size = iokit_prop_number(entry, "Size").unwrap_or(0.0) as u64;
        out.push(RawDisk { name: bsd, model, size, read, written });
    });
    if out.is_empty() {
        // Fallback: synthesize from mounted volumes (no I/O counters).
        out.push(RawDisk { name: "disk0".into(), model: "Internal storage".into(), size: 0, read: 0, written: 0 });
    }
    out
}

struct Mount {
    dev: String,
    path: String,
    fstype: String,
    total: u64,
    free: u64,
}

fn mounts() -> Vec<Mount> {
    let mut out = Vec::new();
    unsafe {
        let mut buf: *mut core::ffi::c_void = core::ptr::null_mut();
        let n = getmntinfo(&mut buf, MNT_NOWAIT);
        if n <= 0 || buf.is_null() {
            return out;
        }
        let base = buf as *const u8;
        for i in 0..n as usize {
            let rec = core::slice::from_raw_parts(base.add(i * STATFS_STRIDE), STATFS_STRIDE);
            let bsize = r_u64(rec, STATFS_BSIZE).max(1);
            let path = r_cstr(rec, STATFS_MNTONNAME);
            if path.is_empty() {
                continue;
            }
            out.push(Mount {
                dev: r_cstr(rec, STATFS_MNTFROMNAME),
                path,
                fstype: r_cstr(rec, STATFS_FSTYPENAME),
                total: r_u64(rec, STATFS_BLOCKS) * bsize,
                free: r_u64(rec, STATFS_BAVAIL) * bsize,
            });
        }
    }
    out
}

/// Strip "/dev/disk3s1s1" → "disk3".
fn whole_disk(dev: &str) -> String {
    let d = dev.strip_prefix("/dev/").unwrap_or(dev);
    let mut end = d.len();
    for (i, c) in d.char_indices() {
        if c == 's' && i > 0 && d.as_bytes()[i - 1].is_ascii_digit() && d[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < d.len() {
            end = i;
            break;
        }
    }
    d[..end].to_string()
}

pub fn disks(prev: &mut HashMap<String, (u64, u64)>, dt_secs: f64) -> Vec<DiskInfo> {
    let mnts = mounts();
    let mut out = Vec::new();
    for d in raw_disks() {
        let (rb, wb) = match prev.get(&d.name) {
            Some(&(pr, pw)) if dt_secs > 0.0 => ((d.read.saturating_sub(pr)) as f64 / dt_secs, (d.written.saturating_sub(pw)) as f64 / dt_secs),
            _ => (0.0, 0.0),
        };
        prev.insert(d.name.clone(), (d.read, d.written));
        // attach the root-ish volume of this disk
        let vol = mnts
            .iter()
            .filter(|m| whole_disk(&m.dev) == d.name && !m.path.starts_with("/System/Volumes/VM"))
            .max_by_key(|m| if m.path == "/" { usize::MAX } else { 0 });
        let (mount, fs, vt, vf) = vol.map(|m| (m.path.clone(), m.fstype.clone(), m.total, m.free)).unwrap_or_default();
        out.push(DiskInfo {
            name: d.name,
            model: d.model,
            size_bytes: if d.size > 0 { d.size } else { vt },
            read_bps: rb,
            write_bps: wb,
            total_read: d.read,
            total_written: d.written,
            mount,
            fs_type: fs,
            vol_total: vt,
            vol_free: vf,
        });
    }
    out
}

// --------------------------------------------------------------- networks

struct RawIf {
    name: String,
    rx: u64,
    tx: u64,
    mtu: u32,
    up: bool,
}

fn raw_ifs() -> Vec<RawIf> {
    let mut out: Vec<RawIf> = Vec::new();
    unsafe {
        let mut head: *mut IfAddrs = core::ptr::null_mut();
        if getifaddrs(&mut head) != 0 || head.is_null() {
            return out;
        }
        let mut cur = head;
        while !cur.is_null() {
            let ifa = &*cur;
            if !ifa.ifa_addr.is_null() && *(ifa.ifa_addr as *const u8).add(1) == AF_LINK && !ifa.ifa_data.is_null() {
                let data = core::slice::from_raw_parts(ifa.ifa_data as *const u8, 80);
                let name = {
                    let mut s = Vec::new();
                    let mut p = ifa.ifa_name as *const u8;
                    while *p != 0 {
                        s.push(*p);
                        p = p.add(1);
                    }
                    String::from_utf8_lossy(&s).into_owned()
                };
                if !name.starts_with("lo") {
                    let rx = r_u32(data, IFD_IBYTES) as u64;
                    let tx = r_u32(data, IFD_OBYTES) as u64;
                    if let Some(existing) = out.iter_mut().find(|i| i.name == name) {
                        existing.rx = existing.rx.max(rx);
                        existing.tx = existing.tx.max(tx);
                    } else {
                        out.push(RawIf { name, rx, tx, mtu: r_u32(data, IFD_MTU), up: ifa.ifa_flags & 0x1 != 0 });
                    }
                }
            }
            cur = ifa.ifa_next;
        }
        freeifaddrs(head);
    }
    out
}

pub fn networks(prev: &mut HashMap<String, (u64, u64)>, dt_secs: f64) -> Vec<NetIfInfo> {
    raw_ifs()
        .into_iter()
        .map(|i| {
            let (rb, tb) = match prev.get(&i.name) {
                Some(&(pr, pt)) if dt_secs > 0.0 => {
                    // 32-bit counter wrap-around protection
                    let dr = if i.rx >= pr { i.rx - pr } else { u32::MAX as u64 + i.rx - pr };
                    let dt = if i.tx >= pt { i.tx - pt } else { u32::MAX as u64 + i.tx - pt };
                    (dr as f64 / dt_secs, dt as f64 / dt_secs)
                }
                _ => (0.0, 0.0),
            };
            prev.insert(i.name.clone(), (i.rx, i.tx));
            let display = match i.name.as_str() {
                n if n.starts_with("en") => format!("Ethernet/Wi-Fi ({n})"),
                n if n.starts_with("utun") => format!("VPN tunnel ({n})"),
                n if n.starts_with("awdl") => format!("AirDrop ({n})"),
                n if n.starts_with("llw") => format!("Low-latency WLAN ({n})"),
                n if n.starts_with("bridge") => format!("Bridge ({n})"),
                n => n.to_string(),
            };
            NetIfInfo { name: i.name, display_name: display, rx_bps: rb, tx_bps: tb, total_rx: i.rx, total_tx: i.tx, is_up: i.up, mtu: i.mtu }
        })
        .collect()
}
