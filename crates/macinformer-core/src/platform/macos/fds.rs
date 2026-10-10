//! System-wide file-descriptor walk — the engine behind:
//!   * Network tab (TCP/UDP connections with owning process)
//!   * Find Open Files (Ctrl+F — SI's "Find Handles or DLLs")
//!
//! Same technique as `lsof`: PROC_PIDLISTFDS + PROC_PIDFDSOCKETINFO /
//! PROC_PIDFDVNODEPATHINFO for every pid.

use super::ffi::*;
use crate::types::*;
use std::collections::HashMap;

fn pid_names() -> HashMap<i32, String> {
    let mut map = HashMap::with_capacity(512);
    let mut n = unsafe { proc_listpids(PROC_ALL_PIDS, 0, core::ptr::null_mut(), 0) };
    if n <= 0 {
        return map;
    }
    n += 64;
    let mut pids = vec![0i32; (n as usize) / 4 + 64];
    let got = unsafe { proc_listpids(PROC_ALL_PIDS, 0, pids.as_mut_ptr() as *mut _, (pids.len() * 4) as i32) };
    if got <= 0 {
        return map;
    }
    pids.truncate(got as usize / 4);
    let mut buf = vec![0u8; BSDINFO_SIZE + 16];
    for pid in pids.into_iter().filter(|&p| p > 0) {
        let r = unsafe { proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
        if r as usize >= BSDINFO_SIZE {
            let mut name = r_cstr(&buf, PBI_NAME);
            if name.is_empty() {
                name = r_cstr(&buf, PBI_COMM);
            }
            map.insert(pid, name);
        }
    }
    map
}

/// (fd, fdtype) pairs for one process.
fn pid_fds(pid: i32) -> Vec<(i32, u32)> {
    let mut cap = 4096usize;
    for _ in 0..6 {
        let mut buf = vec![0u8; cap];
        let r = unsafe { proc_pidinfo(pid, PROC_PIDLISTFDS, 0, buf.as_mut_ptr() as *mut _, cap as i32) };
        if r <= 0 {
            return Vec::new();
        }
        if r as usize >= cap {
            cap *= 4; // buffer full — grow and retry
            continue;
        }
        return buf[..r as usize]
            .as_chunks::<FDINFO_SIZE>().0.iter()
            .map(|c| (r_i32(c, 0), r_u32(c, 4)))
            .collect();
    }
    Vec::new()
}

fn socket_info(pid: i32, fd: i32) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; SOCKET_FDINFO_SIZE + 64];
    let r = unsafe { proc_pidfdinfo(pid, fd, PROC_PIDFDSOCKETINFO, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
    (r as usize >= SFI_PROTO + 4).then_some(buf)
}

fn vnode_path(pid: i32, fd: i32) -> Option<String> {
    let mut buf = vec![0u8; 2048];
    let r = unsafe { proc_pidfdinfo(pid, fd, PROC_PIDFDVNODEPATHINFO, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
    if r as usize > VNODEPATH_FD_PATH_OFFSET {
        let p = r_cstr(&buf, VNODEPATH_FD_PATH_OFFSET);
        if !p.is_empty() {
            return Some(p);
        }
    }
    None
}

fn port_be(buf: &[u8], off: usize) -> u16 {
    u16::from_be(r_u16(buf, off))
}

fn fmt_addr(buf: &[u8], off: usize, vflag: u8) -> String {
    if vflag & INSI_IPV4 != 0 {
        let a = &buf[off + 12..off + 16];
        format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
    } else if vflag & INSI_IPV6 != 0 {
        let b = &buf[off..off + 16];
        let groups: Vec<String> = (0..8).map(|i| format!("{:x}", u16::from_be_bytes([b[i * 2], b[i * 2 + 1]]))).collect();
        groups.join(":")
    } else {
        String::new()
    }
}

fn tcp_state(s: i32) -> &'static str {
    match s {
        0 => "Closed",
        1 => "Listen",
        2 => "Syn sent",
        3 => "Syn received",
        4 => "Established",
        5 => "Close wait",
        6 => "Fin wait 1",
        7 => "Closing",
        8 => "Last ack",
        9 => "Fin wait 2",
        10 => "Time wait",
        _ => "Unknown",
    }
}

struct ParsedSocket {
    proto: Option<Protocol>,
    local: String,
    lport: u16,
    remote: String,
    rport: u16,
    state: String,
}

fn parse_socket(buf: &[u8]) -> Option<ParsedSocket> {
    if r_i32(buf, SFI_KIND) != SOCKINFO_IN {
        return None;
    }
    let proto_num = r_i32(buf, SFI_PROTOCOL);
    let vflag = r_u8(buf, SFI_PROTO + INSI_VFLAG);
    let v6 = vflag & INSI_IPV6 != 0 && vflag & INSI_IPV4 == 0;
    let proto = match (proto_num, v6) {
        (IPPROTO_TCP, false) => Protocol::Tcp4,
        (IPPROTO_TCP, true) => Protocol::Tcp6,
        (IPPROTO_UDP, false) => Protocol::Udp4,
        (IPPROTO_UDP, true) => Protocol::Udp6,
        _ => return None,
    };
    let local = fmt_addr(buf, SFI_PROTO + INSI_LADDR, vflag);
    let remote = fmt_addr(buf, SFI_PROTO + INSI_FADDR, vflag);
    let lport = port_be(buf, SFI_PROTO + INSI_LPORT);
    let rport = port_be(buf, SFI_PROTO + INSI_FPORT);
    let state = if proto_num == IPPROTO_TCP {
        tcp_state(r_i32(buf, SFI_PROTO + TCPSI_STATE)).to_string()
    } else {
        String::new()
    };
    Some(ParsedSocket { proto: Some(proto), local, lport, remote, rport, state })
}

/// Network tab: every INET socket with its owning process.
pub fn connections() -> Vec<Connection> {
    let names = pid_names();
    let mut out = Vec::new();
    for (&pid, pname) in &names {
        for (fd, fdt) in pid_fds(pid) {
            if fdt != PROX_FDTYPE_SOCKET {
                continue;
            }
            let Some(buf) = socket_info(pid, fd) else { continue };
            let Some(s) = parse_socket(&buf) else { continue };
            out.push(Connection {
                pid,
                process_name: pname.clone(),
                protocol: s.proto.unwrap(),
                local_addr: s.local,
                local_port: s.lport,
                remote_addr: s.remote,
                remote_port: s.rport,
                state: s.state,
            });
        }
    }
    out.sort_by_key(|c| (c.process_name.clone(), c.protocol as u8, c.local_port));
    out
}

/// Find Open Files (Ctrl+F): every descriptor of every process.
pub fn open_files() -> Vec<OpenFile> {
    let names = pid_names();
    let mut out = Vec::new();
    for (&pid, pname) in &names {
        for (fd, fdt) in pid_fds(pid) {
            let (kind, target) = match fdt {
                PROX_FDTYPE_VNODE => (FdKind::File, vnode_path(pid, fd).unwrap_or_else(|| "(vnode)".into())),
                PROX_FDTYPE_SOCKET => {
                    let t = socket_info(pid, fd)
                        .and_then(|b| parse_socket(&b))
                        .map(|s| {
                            if s.rport > 0 {
                                format!("{}:{} -> {}:{}", s.local, s.lport, s.remote, s.rport)
                            } else {
                                format!("{}:{}", s.local, s.lport)
                            }
                        })
                        .unwrap_or_else(|| "(socket)".into());
                    (FdKind::Socket, t)
                }
                PROX_FDTYPE_PIPE => (FdKind::Pipe, "(pipe)".into()),
                PROX_FDTYPE_KQUEUE => (FdKind::Kqueue, "(kqueue)".into()),
                PROX_FDTYPE_PSXSHM => (FdKind::PsxShm, "(posix shm)".into()),
                PROX_FDTYPE_PSXSEM => (FdKind::PsxSem, "(posix sem)".into()),
                PROX_FDTYPE_FSEVENTS => (FdKind::Fsevents, "(fsevents)".into()),
                PROX_FDTYPE_GUARDED => (FdKind::Guarded, "(guarded)".into()),
                _ => (FdKind::Other, format!("(type {fdt})")),
            };
            out.push(OpenFile { pid, process_name: pname.clone(), fd, kind, target });
        }
    }
    out
}

/// Open files of a single process (Properties > Open Files page).
pub fn open_files_of(pid: i32) -> Vec<OpenFile> {
    let names = pid_names();
    let pname = names.get(&pid).cloned().unwrap_or_default();
    pid_fds(pid)
        .into_iter()
        .map(|(fd, fdt)| {
            let (kind, target) = match fdt {
                PROX_FDTYPE_VNODE => (FdKind::File, vnode_path(pid, fd).unwrap_or_else(|| "(vnode)".into())),
                PROX_FDTYPE_SOCKET => (
                    FdKind::Socket,
                    socket_info(pid, fd)
                        .and_then(|b| parse_socket(&b))
                        .map(|s| format!("{}:{} -> {}:{}", s.local, s.lport, s.remote, s.rport))
                        .unwrap_or_else(|| "(socket)".into()),
                ),
                PROX_FDTYPE_PIPE => (FdKind::Pipe, "(pipe)".into()),
                PROX_FDTYPE_KQUEUE => (FdKind::Kqueue, "(kqueue)".into()),
                _ => (FdKind::Other, format!("(type {fdt})")),
            };
            OpenFile { pid, process_name: pname.clone(), fd, kind, target }
        })
        .collect()
}
