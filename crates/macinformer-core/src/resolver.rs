//! Reverse-DNS resolution for the Network and NetLog tabs (SI's "Resolve
//! hostnames" toggle). Non-blocking: a worker thread resolves addresses in
//! the background and the UI just reads whatever is cached — the interface
//! never stalls on a slow resolver. Negative results are cached briefly so
//! unresolvable addresses don't hammer DNS every refresh.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const NEG_CACHE_TTL: Duration = Duration::from_secs(60);
const POS_CACHE_TTL: Duration = Duration::from_secs(600);

struct Entry {
    name: Option<String>,
    at: Instant,
}

pub struct Resolver {
    pub enabled: AtomicBool,
    cache: Arc<Mutex<HashMap<String, Entry>>>,
    tx: Sender<String>,
}

fn global() -> &'static Resolver {
    static R: OnceLock<Resolver> = OnceLock::new();
    R.get_or_init(|| {
        let (tx, rx) = channel::<String>();
        let cache: Arc<Mutex<HashMap<String, Entry>>> = Arc::new(Mutex::new(HashMap::new()));
        let worker_cache = Arc::clone(&cache);
        std::thread::spawn(move || resolver_worker(rx, worker_cache));
        Resolver { enabled: AtomicBool::new(true), cache, tx }
    })
}

fn resolver_worker(rx: Receiver<String>, cache: Arc<Mutex<HashMap<String, Entry>>>) {
    while let Ok(ip) = rx.recv() {
        let name = resolve_blocking(&ip);
        if let Ok(mut c) = cache.lock() {
            c.insert(ip, Entry { name, at: Instant::now() });
        }
    }
}

/// Blocking PTR lookup via getaddrinfo(NI_NAMEREQD) — runs on the worker only.
fn resolve_blocking(ip: &str) -> Option<String> {
    use core::ffi::c_int;
    #[repr(C)]
    struct AddrInfo {
        ai_flags: c_int,
        ai_family: c_int,
        ai_socktype: c_int,
        ai_protocol: c_int,
        ai_addrlen: u32,
        ai_canonname: *mut i8,
        ai_addr: *mut SockAddrStorage,
        ai_next: *mut AddrInfo,
    }
    #[repr(C)]
    struct SockAddrStorage {
        ss_len: u8,
        ss_family: u8,
        data: [u8; 126],
    }
    unsafe extern "C" {
        fn getaddrinfo(node: *const i8, service: *const i8, hints: *const AddrInfo, res: *mut *mut AddrInfo) -> c_int;
        fn getnameinfo(sa: *const SockAddrStorage, salen: u32, host: *mut i8, hostlen: u32, serv: *mut i8, servlen: u32, flags: c_int) -> c_int;
        fn freeaddrinfo(res: *mut AddrInfo);
    }
    const NI_NAMEREQD: c_int = if cfg!(target_os = "macos") { 4 } else { 8 };
    const NI_MAXHOST: usize = 1025;
    let c_ip = std::ffi::CString::new(ip).ok()?;
    unsafe {
        let mut hints: AddrInfo = core::mem::zeroed();
        hints.ai_family = 0; // AF_UNSPEC
        hints.ai_socktype = 1; // SOCK_STREAM
        hints.ai_flags = 1; // AI_NUMERICHOST — parse, don't resolve forward
        let mut res: *mut AddrInfo = core::ptr::null_mut();
        if getaddrinfo(c_ip.as_ptr(), core::ptr::null(), &hints, &mut res) != 0 || res.is_null() {
            return None;
        }
        let mut host = [0i8; NI_MAXHOST];
        let rc = getnameinfo((*res).ai_addr, (*res).ai_addrlen, host.as_mut_ptr(), NI_MAXHOST as u32, core::ptr::null_mut(), 0, NI_NAMEREQD);
        freeaddrinfo(res);
        if rc != 0 {
            return None;
        }
        let s = core::ffi::CStr::from_ptr(host.as_ptr()).to_string_lossy().into_owned();
        if s.is_empty() { None } else { Some(s) }
    }
}

/// Is this a literal IP address (v4 or v6)?
pub fn is_ip_literal(s: &str) -> bool {
    if s.is_empty() || s == "*" || s == "0.0.0.0" || s == "::" || s == "127.0.0.1" || s == "::1" {
        return false;
    }
    s.parse::<std::net::IpAddr>().is_ok()
}

/// Non-blocking lookup: returns the cached hostname if fresh, otherwise
/// queues the address for background resolution and returns None (the UI
/// keeps showing the IP until the name arrives).
pub fn lookup(ip: &str) -> Option<String> {
    let r = global();
    if !r.enabled.load(Ordering::Relaxed) || !is_ip_literal(ip) {
        return None;
    }
    {
        let c = r.cache.lock().ok()?;
        if let Some(e) = c.get(ip) {
            let ttl = if e.name.is_some() { POS_CACHE_TTL } else { NEG_CACHE_TTL };
            if e.at.elapsed() < ttl {
                return e.name.clone();
            }
        }
    }
    // not cached (or stale): queue and show IP for now
    let _ = r.tx.send(ip.to_string());
    None
}

pub fn set_enabled(on: bool) {
    global().enabled.store(on, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    global().enabled.load(Ordering::Relaxed)
}

/// Format an endpoint for display: "host:port" when resolved, else "ip:port".
pub fn endpoint(ip: &str, port: u16) -> String {
    if port == 0 {
        return if ip.is_empty() { "*:*".into() } else { format!("{ip}:*") };
    }
    match lookup(ip) {
        Some(name) => format!("{name}:{port}"),
        None => format!("{ip}:{port}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_literal_detection() {
        assert!(is_ip_literal("93.184.216.34"));
        assert!(is_ip_literal("2606:2800:220:1:248:1893:25c8:1946"));
        assert!(!is_ip_literal("example.com"));
        assert!(!is_ip_literal("127.0.0.1")); // loopback: pointless to resolve
        assert!(!is_ip_literal("*"));
        assert!(!is_ip_literal("0.0.0.0"));
    }

    #[test]
    fn endpoint_formats_with_port_zero() {
        set_enabled(false);
        assert_eq!(endpoint("", 0), "*:*");
        assert_eq!(endpoint("10.0.0.1", 0), "10.0.0.1:*");
        assert_eq!(endpoint("10.0.0.1", 443), "10.0.0.1:443");
    }

    #[test]
    fn disabled_resolver_shows_ip() {
        set_enabled(false);
        assert!(!is_enabled());
        assert_eq!(lookup("93.184.216.34"), None);
        set_enabled(true);
    }
}
