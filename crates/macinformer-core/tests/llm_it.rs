//! End-to-end tests of the AI integration: a real mock OpenAI-compatible
//! server on 127.0.0.1:0 verifies the request shape MacInformer sends, and a
//! full `--server` instance answers POST /api/chat through it.

use macinformer_core::server::{run_on, ServerConfig};
use macinformer_core::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

// --------------------------------------------------------------- mock LLM

/// One captured request hitting the mock model server.
struct Captured {
    method: String,
    path: String,
    authorization: Option<String>,
    body: String,
}

/// Spawn a mock OpenAI server that answers `reply_text` to every POST and
/// reports what it received through the returned channel.
fn mock_openai(reply_text: &'static str) -> (String, mpsc::Receiver<Captured>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let tx = tx.clone();
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let mut cl = 0usize;
                let mut head_end = None;
                loop {
                    if head_end.is_none() {
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            head_end = Some(p + 4);
                            let head = String::from_utf8_lossy(&buf[..p]).into_owned();
                            for line in head.split("\r\n") {
                                if let Some((k, v)) = line.split_once(':') {
                                    if k.trim().eq_ignore_ascii_case("content-length") {
                                        cl = v.trim().parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(he) = head_end {
                        if buf.len() >= he + cl {
                            break;
                        }
                    }
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                    if buf.len() > 4_000_000 {
                        break;
                    }
                }
                let Some(he) = head_end else { return };
                let head = String::from_utf8_lossy(&buf[..he]).into_owned();
                let mut lines = head.split("\r\n");
                let first = lines.next().unwrap_or("");
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let authorization = lines
                    .clone()
                    .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("authorization")).map(|(_, v)| v.trim().to_string()));
                let body = String::from_utf8_lossy(&buf[he..buf.len().min(he + cl)]).into_owned();
                let _ = tx.send(Captured { method, path, authorization, body });
                let reply = format!(
                    "{{\"id\":\"chatcmpl-mock\",\"choices\":[{{\"index\":0,\"message\":{{\"role\":\"assistant\",\"content\":{}}}}}]}}",
                    json_str(reply_text)
                );
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    (format!("http://{addr}/v1/chat/completions"), rx)
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ------------------------------------------------------------------ tests

#[test]
fn assistant_roundtrip_against_mock_openai() {
    let (url, rx) = mock_openai("Your Mac is fine.\nACTION: {\"action\":\"switch_tab\",\"tab\":\"netlog\"}");
    let provider = llm::resolve("gguf", Some("test-model"), Some(&url), Some("sk-mock")).unwrap();
    let mut a = llm::Assistant::new(provider);
    let reply = a.ask("CPU 12%, memory 40%.", "how is my mac?").unwrap();
    assert_eq!(reply.text, "Your Mac is fine.");
    assert_eq!(reply.actions, vec![llm::Action::SwitchTab("netlog".into())]);

    let cap = rx.recv_timeout(Duration::from_secs(3)).expect("mock saw a request");
    assert_eq!(cap.method, "POST");
    assert_eq!(cap.path, "/v1/chat/completions");
    assert_eq!(cap.authorization.as_deref(), Some("Bearer sk-mock"));
    let v = json::parse(&cap.body).unwrap();
    assert_eq!(v.get("model").unwrap().as_str(), Some("test-model"));
    let msgs = v.get("messages").unwrap().as_arr().unwrap();
    assert_eq!(msgs[0].get("role").unwrap().as_str(), Some("system"));
    assert!(msgs[0].get("content").unwrap().as_str().unwrap().contains("CPU 12%, memory 40%."), "system prompt carries live context");
    assert!(msgs[0].get("content").unwrap().as_str().unwrap().contains("ACTION:"), "system prompt documents actions");
    assert_eq!(msgs[1].get("content").unwrap().as_str(), Some("how is my mac?"));

    // second turn: history grows, and the mock sees it
    let _ = a.ask("CPU 13%.", "and now?").unwrap();
    let cap2 = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let v2 = json::parse(&cap2.body).unwrap();
    let m2 = v2.get("messages").unwrap().as_arr().unwrap();
    assert_eq!(m2.len(), 4, "system + 3 history messages: {m2:?}");
    assert_eq!(m2[1].get("content").unwrap().as_str(), Some("how is my mac?"));
    assert_eq!(m2[2].get("content").unwrap().as_str(), Some("Your Mac is fine."));
}

struct FakeCollector;

impl Collector for FakeCollector {
    fn snapshot(&mut self) -> Snapshot {
        let mut s = Snapshot::default();
        s.cpu.total = 12.5;
        s.cpu.logical_count = 8;
        s.cpu.brand = "FakeM2".into();
        s.memory.total = 16 * 1024 * 1024 * 1024;
        s.memory.used = 6 * 1024 * 1024 * 1024;
        s.process_total = 1;
        s.processes = vec![ProcessInfo { pid: 7, name: "kernel_task".into(), cpu_percent: 3.0, memory_bytes: 1024, ..Default::default() }];
        s
    }
    fn connections(&mut self) -> Vec<Connection> {
        Vec::new()
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

fn http_post(addr: &str, target: &str, body: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(
        format!("POST {target} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes(),
    )
    .unwrap();
    let mut buf = Vec::new();
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = s.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

#[test]
fn server_chat_endpoint_full_stack() {
    let (url, rx) = mock_openai("CPU looks calm at 12.5%.\nACTION: {\"action\":\"filter\",\"section\":\"processes\",\"query\":\"safari\"}");
    let provider = llm::resolve("mlx", Some("mock-mlx"), Some(&url), None).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let cfg = ServerConfig { listen: addr.clone(), refresh_ms: 200, ai: Some(provider), ..Default::default() };
    std::thread::spawn(move || run_on(listener, Box::new(FakeCollector), cfg));
    std::thread::sleep(Duration::from_millis(600));

    // health advertises the provider
    let mut s = TcpStream::connect(&addr).unwrap();
    s.write_all(format!("GET /api/health HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = s.read_to_end(&mut buf);
    let health = String::from_utf8_lossy(&buf).into_owned();
    assert!(health.contains("\"provider\":\"MLX (local) (mock-mlx)\""), "health: {health}");

    // bad body is rejected cleanly
    let bad = http_post(&addr, "/api/chat", "not json");
    assert!(body_of(&bad).contains("\"error\""), "bad body: {bad}");

    // real question flows through the mock and back with actions
    let resp = http_post(&addr, "/api/chat", "{\"question\":\"is cpu ok?\"}");
    let b = body_of(&resp);
    let v = json::parse(b).unwrap_or_else(|e| panic!("chat reply not JSON: {e}: {b}"));
    assert_eq!(v.get("reply").unwrap().as_str(), Some("CPU looks calm at 12.5%."));
    let actions = v.get("actions").unwrap().as_arr().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].get("action").unwrap().as_str(), Some("filter"));
    assert_eq!(actions[0].get("query").unwrap().as_str(), Some("safari"));

    // the model received the live digest from the producer loop
    let cap = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let v = json::parse(&cap.body).unwrap();
    let sys = v.get("messages").unwrap().idx(0).unwrap().get("content").unwrap().as_str().unwrap();
    assert!(sys.contains("FakeM2"), "digest reached the model: {sys}");
    assert!(sys.contains("kernel_task"), "digest names processes: {sys}");
}

#[test]
fn server_chat_without_provider_returns_error_not_crash() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || run_on(listener, Box::new(FakeCollector), ServerConfig::default()));
    std::thread::sleep(Duration::from_millis(400));
    let resp = http_post(&addr, "/api/chat", "{\"question\":\"hi\"}");
    assert!(body_of(&resp).contains("AI not configured"), "{resp}");
}
