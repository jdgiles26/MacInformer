//! macinformer — System Informer's feature set for macOS (Apple Silicon).
//!
//! Usage mirrors System Informer's command-line flags:
//!   macinformer                  interactive TUI (Processes tab)
//!   macinformer --tab services   open a specific tab
//!   macinformer --sysinfo        open System Information graphs
//!   macinformer --selectpid 4242 select a process on start
//!   macinformer --elevate        re-launch with root privileges (sudo)
//!   macinformer --dump [WxH]     render one frame to stdout (CI/docs)

use macinformer_core::system_collector;
use macinformer_tui::{App, AppConfig};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn help() {
    println!(
        "MacInformer {VERSION} — System Informer's exact feature set, native for macOS (Apple Silicon)\n\
         \n\
         USAGE:\n    macinformer [OPTIONS]\n\
         \n\
         OPTIONS:\n\
         \x20   --tab <NAME>     processes | services | network | netlog | disk | events\n\
         \x20   --sysinfo        start in the System Information graphs view (Ctrl+I)\n\
         \x20   --selectpid <N>  select process <N> on start\n\
         \x20   --refresh <MS>   refresh interval in ms (default 1000)\n\
         \x20   --no-tree        flat process list (default: tree)\n\
         \x20   --netlog <FILE>  stream recorded connection events to FILE as JSONL\n\
         \x20   --server         headless agent: HTTP + WebSocket JSON stream for the\n\
         \x20                    iOS companion / browser dashboard (default 127.0.0.1:7270)\n\
         \x20   --listen <ADDR>  server bind address (use 0.0.0.0:7270 for LAN/iPhone)\n\
         \x20   --token <SECRET> require ?token=SECRET on /ws and /api/* (LAN recommended)\n\
         \x20   --elevate        re-exec with root privileges for full fidelity\n\
         \x20   --ai <NAME>      AI assistant: claude | grok | kimi | mlx | gguf | onnx | auto\n\
         \x20                    (keys via ANTHROPIC_API_KEY / XAI_API_KEY / MOONSHOT_API_KEY;\n\
         \x20                    mlx/gguf/onnx talk to a local OpenAI-compatible server)\n\
         \x20   --ai-model <ID>  override the provider's default model\n\
         \x20   --ai-url <URL>   override the provider endpoint\n\
         \x20   --ai-key <KEY>   API key (prefer env vars; flags land in shell history)\n\
         \x20   --report <FILE>  write a standalone HTML report of the current system\n\
         \x20                    state (with AI explanations when --ai is set) and exit\n\
         \x20   --dump [WxH]     print one rendered frame and exit (default 130x38)\n\
         \x20   -h, --help       this help\n\
         \x20   -V, --version    version\n\
         \n\
         See docs/FEATURE_PARITY.md for the System Informer → macOS feature matrix\n\
         and docs/ARCHITECTURE.md for the design."
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = AppConfig::default();
    let mut dump: Option<(usize, usize)> = None;
    let mut elevate = false;
    let mut report: Option<String> = None;
    let mut server: Option<macinformer_core::server::ServerConfig> = None;
    let mut ai: Option<String> = None;
    let mut ai_model: Option<String> = None;
    let mut ai_url: Option<String> = None;
    let mut ai_key: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return help(),
            "-V" | "--version" => return println!("macinformer {VERSION}"),
            "--tab" => {
                i += 1;
                cfg.tab = args.get(i).cloned();
            }
            "--sysinfo" => cfg.sysinfo = true,
            "--selectpid" => {
                i += 1;
                cfg.select_pid = args.get(i).and_then(|s| s.parse().ok());
            }
            "--refresh" => {
                i += 1;
                if let Some(ms) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.refresh_ms = ms;
                    if let Some(s) = server.as_mut() {
                        s.refresh_ms = ms;
                    }
                }
            }
            "--no-tree" => cfg.no_tree = true,
            "--netlog" => {
                i += 1;
                cfg.netlog_path = args.get(i).cloned();
            }
            "--server" => {
                server = Some(macinformer_core::server::ServerConfig { refresh_ms: cfg.refresh_ms, ..Default::default() });
            }
            "--listen" => {
                i += 1;
                if let Some(a) = args.get(i) {
                    server.get_or_insert_with(Default::default).listen = a.clone();
                }
            }
            "--token" => {
                i += 1;
                server.get_or_insert_with(Default::default).token = args.get(i).cloned();
            }
            "--elevate" => elevate = true,
            "--ai" => {
                i += 1;
                ai = args.get(i).cloned();
            }
            "--ai-model" => {
                i += 1;
                ai_model = args.get(i).cloned();
            }
            "--ai-url" => {
                i += 1;
                ai_url = args.get(i).cloned();
            }
            "--ai-key" => {
                i += 1;
                ai_key = args.get(i).cloned();
            }
            "--report" => {
                i += 1;
                report = args.get(i).cloned();
            }
            "--dump" => {
                let (mut w, mut h) = (130usize, 38usize);
                if let Some(v) = args.get(i + 1) {
                    if let Some((ws, hs)) = v.split_once(['x', 'X']) {
                        if let (Ok(w2), Ok(h2)) = (ws.parse(), hs.parse()) {
                            w = w2;
                            h = h2;
                            i += 1;
                        }
                    }
                }
                dump = Some((w, h));
            }
            other => {
                eprintln!("macinformer: unknown option '{other}' (try --help)");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    if elevate && macinformer_core::types::current_uid() != 0 {
        let exe = std::env::current_exe().expect("current exe");
        let passthrough: Vec<&String> = args.iter().filter(|a| a.as_str() != "--elevate").collect();
        let status = std::process::Command::new("sudo").arg(exe).args(&passthrough).status();
        match status {
            Ok(s) => std::process::exit(s.code().unwrap_or(0)),
            Err(e) => {
                eprintln!("macinformer: sudo failed: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(name) = ai.as_deref() {
        use macinformer_core::llm;
        let provider = if name == "auto" {
            llm::auto_detect(ai_model.as_deref(), ai_url.as_deref(), ai_key.as_deref()).ok_or_else(|| {
                "auto-detect found no AI provider: no ANTHROPIC_API_KEY/XAI_API_KEY/MOONSHOT_API_KEY in the environment \
                 and no local model server on 127.0.0.1:8080 or :8000".to_string()
            })
        } else {
            llm::resolve(name, ai_model.as_deref(), ai_url.as_deref(), ai_key.as_deref())
        };
        match provider {
            Ok(p) => {
                eprintln!("macinformer: AI assistant via {}", p.describe());
                cfg.ai = Some(p);
            }
            Err(e) => {
                eprintln!("macinformer: --ai {name}: {e}");
                std::process::exit(2);
            }
        }
    }

    let collector = system_collector();
    if let Some(path) = report {
        if let Err(e) = macinformer_core::report::write_report(collector, cfg.ai.take(), &path) {
            eprintln!("macinformer: report failed: {e}");
            std::process::exit(1);
        }
        println!("report written to {path}");
        return;
    }
    if let Some(mut scfg) = server {
        // headless agent mode — the iOS companion / browser dashboard stream
        scfg.ai = cfg.ai.take(); // POST /api/chat on the dashboard
        if let Err(e) = macinformer_core::server::run(collector, scfg) {
            eprintln!("macinformer: server failed: {e}");
            std::process::exit(1);
        }
        return;
    }
    let mut app = App::new(collector, cfg);
    if let Some((w, h)) = dump {
        // two ticks so rate-based columns (CPU%, I/O) have real deltas
        app.tick();
        std::thread::sleep(std::time::Duration::from_millis(400));
        print!("{}", app.dump_frame(w, h));
        return;
    }
    if let Err(e) = app.run() {
        eprintln!("macinformer: {e}");
        std::process::exit(1);
    }
}
