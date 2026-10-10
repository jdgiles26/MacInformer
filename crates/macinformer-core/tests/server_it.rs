//! End-to-end test of `macinformer --server`: real TCP listener, real HTTP
//! responses, real RFC 6455 WebSocket handshake and frame delivery — against
//! a fake collector so it runs anywhere.

use macinformer_core::server::{run_on, ServerConfig};
use macinformer_core::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

struct FakeCollector {
    tick: u64,
}

impl Collector for FakeCollector {
    fn snapshot(&mut self) -> Snapshot {
        self.tick += 1;
        let mut s = Snapshot::default();
        s.cpu.total = 25.0 + (self.tick % 10) as f32;
        s.cpu.per_core = vec![10.0, 20.0];
        s.cpu.logical_count = 2;
        s.cpu.brand = "FakeCPU".into();
        s.memory.total = 16 * 1024 * 1024 * 1024;
        s.memory.used = 8 * 1024 * 1024 * 1024;
        s.process_total = 1;
        s.processes = vec![ProcessInfo { pid: 4242, name: "testproc".into(), cpu_percent: 7.5, memory_bytes: 4096, ..Default::default() }];
        s
    }
    fn connections(&mut self) -> Vec<Connection> {
        vec![Connection {
            pid: 4242,
            process_name: "testproc".into(),
            protocol: Protocol::Tcp4,
            local_addr: "10.0.0.5".into(),
            local_port: 51000,
            remote_addr: "93.184.216.34".into(),
            remote_port: 443,
            state: "Established".into(),
        }]
    }
    fn open_files(&mut self) -> Vec<OpenFile> {
        Vec::new()
    }
    fn services(&mut self) -> Vec<ServiceInfo> {
        Vec::new()
    }
    fn environment(&self, _pid: i32) -> Vec<(String, String)> {
        Vec::new()
    }
    fn process_detail(&mut self, _pid: i32) -> Option<ProcessDetail> {
        None
    }
    fn send_signal(&self, _pid: i32, _sig: i32) -> Result<(), String> {
        Ok(())
    }
    fn set_nice(&self, _pid: i32, _nice: i32) -> Result<(), String> {
        Ok(())
    }
    fn service_action(&self, _label: &str, _action: ServiceAction) -> Result<String, String> {
        Ok("ok".into())
    }
    fn capability_notes(&self) -> Vec<String> {
        Vec::new()
    }
    fn is_root(&self) -> bool {
        false
    }
}

/// Start the server on an ephemeral port in a background thread.
fn start_server(cfg: ServerConfig) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        run_on(listener, Box::new(FakeCollector { tick: 0 }), cfg);
    });
    // give the producer thread a moment to publish its first snapshot
    std::thread::sleep(Duration::from_millis(600));
    addr
}

fn http_get(addr: &str, target: &str, extra_headers: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET {target} HTTP/1.1\r\nHost: {addr}\r\n{extra_headers}\r\n").as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = s.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Minimal JSON structural validity check (balanced braces/brackets/quotes).
fn json_valid(s: &str) -> bool {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for ch in s.chars() {
        if esc {
            esc = false;
        } else if in_str && ch == '\\' {
            esc = true;
        } else if ch == '"' {
            in_str = !in_str;
        } else if !in_str && (ch == '{' || ch == '[') {
            depth += 1;
        } else if !in_str && (ch == '}' || ch == ']') {
            depth -= 1;
            if depth < 0 {
                return false;
            }
        }
    }
    depth == 0 && !in_str
}

#[test]
fn dashboard_and_rest_endpoints() {
    let addr = start_server(ServerConfig { listen: "127.0.0.1:0".into(), token: None, refresh_ms: 300, ..Default::default() });

    let dash = http_get(&addr, "/", "");
    assert!(dash.starts_with("HTTP/1.1 200"), "dashboard status: {}", &dash[..30.min(dash.len())]);
    assert!(dash.contains("MacInformer"));
    assert!(dash.contains("/ws"));

    let snap = http_get(&addr, "/api/snapshot", "");
    assert!(snap.starts_with("HTTP/1.1 200"));
    let b = body_of(&snap);
    assert!(json_valid(b), "snapshot body not valid JSON: {b}");
    assert!(b.contains("\"name\":\"testproc\""), "process in snapshot: {b}");
    assert!(b.contains("\"cpu_total\":"));
    assert!(b.contains("\"connections\":[{\"pid\":4242"));

    let net = http_get(&addr, "/api/netlog", "");
    assert!(net.starts_with("HTTP/1.1 200"));
    let nb = body_of(&net);
    assert!(json_valid(nb));
    assert!(nb.contains("\"event\":\"opened\""), "recorder event in netlog: {nb}");

    let ev = http_get(&addr, "/api/events", "");
    assert!(ev.starts_with("HTTP/1.1 200"));
    assert!(json_valid(body_of(&ev)));

    let not_found = http_get(&addr, "/nope", "");
    assert!(not_found.starts_with("HTTP/1.1 404"));
}

#[test]
fn token_gate_enforced() {
    let addr = start_server(ServerConfig { listen: "127.0.0.1:0".into(), token: Some("s3cret".into()), refresh_ms: 300, ..Default::default() });

    let denied = http_get(&addr, "/api/snapshot", "");
    assert!(denied.starts_with("HTTP/1.1 403"), "expected 403, got {}", &denied[..40.min(denied.len())]);

    let ok = http_get(&addr, "/api/snapshot?token=s3cret", "");
    assert!(ok.starts_with("HTTP/1.1 200"));

    // dashboard itself stays open (it carries no data; the WS upgrade is gated)
    let dash = http_get(&addr, "/", "");
    assert!(dash.starts_with("HTTP/1.1 200"));
}

#[test]
fn websocket_handshake_and_stream() {
    let addr = start_server(ServerConfig { listen: "127.0.0.1:0".into(), token: None, refresh_ms: 300, ..Default::default() });
    let mut s = TcpStream::connect(&addr).unwrap();
    s.write_all(
        format!(
            "GET /ws HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        .as_bytes(),
    )
    .unwrap();
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));

    // read the 101 response
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = s.read(&mut tmp).unwrap();
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    assert!(head.starts_with("HTTP/1.1 101"), "upgrade: {}", &head[..60.min(head.len())]);
    assert!(head.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="), "accept key in: {head}");

    // after \r\n\r\n, remaining bytes (if any) start the first frame;
    // keep reading until we have a full frame
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let mut rest = buf[split..].to_vec();
    loop {
        if rest.len() >= 2 {
            let len0 = (rest[1] & 0x7f) as usize;
            let (hdr, payload_len) = if len0 < 126 {
                (2, len0)
            } else if len0 == 126 && rest.len() >= 4 {
                (4, u16::from_be_bytes([rest[2], rest[3]]) as usize)
            } else if len0 == 127 && rest.len() >= 10 {
                (10, u64::from_be_bytes(rest[2..10].try_into().unwrap()) as usize)
            } else {
                (usize::MAX, usize::MAX) // need more header bytes
            };
            if hdr != usize::MAX && rest.len() >= hdr + payload_len {
                assert_eq!(rest[0], 0x81, "text frame with FIN");
                let payload = String::from_utf8_lossy(&rest[hdr..hdr + payload_len]).into_owned();
                assert!(json_valid(&payload), "ws payload valid JSON: {payload}");
                assert!(payload.contains("\"name\":\"testproc\""));
                return;
            }
        }
        let n = s.read(&mut tmp).unwrap();
        assert!(n > 0, "connection closed before frame");
        rest.extend_from_slice(&tmp[..n]);
    }
}
