# MacInformer

**System Informer's feature set, ported natively to macOS (Apple Silicon) in Rust.**

Reference target: **Mac mini M2** (`aarch64-apple-darwin`, macOS 13+). Zero external
dependencies — every metric is read through direct libproc / Mach / IOKit / sysctl FFI,
mirroring System Informer's own dependency-light C design.

![Processes tab](docs/screenshot-processes.png)
![System Information](docs/screenshot-sysinfo.png)
![NetLog connection recorder](docs/screenshot-netlog.png)
![Events stream](docs/screenshot-events.png)
![Threshold alerts](docs/screenshot-alerts.png)
![Server dashboard](docs/screenshot-server.png)

> Screenshots above are genuine captures of the app running (recorded through a real PTY
> against the Linux fallback collector used for CI; on macOS the same UI is fed by the
> libproc/Mach/IOKit collectors). The dashboard shot is a headless-browser capture of
> `macinformer --server` streaming live over WebSocket — the same page an iPhone opens.

## Features (parity with winsiderss/systeminformer)

- **Processes tab** — tree/flat views, 10 sortable columns (CPU, private bytes, threads,
  FDs, I/O rates…), SI color highlighting (system=root / own / new / dead / suspended),
  live search (Ctrl+K), actions menu (SIGTERM/SIGKILL/SIGSTOP/SIGCONT, nice ±1),
  process **Properties** (General / Statistics / Performance / Open Files / Environment).
- **Services tab** — `launchd` daemons & agents with runtime state, flags
  (RunAtLoad/KeepAlive/Disabled), and start/stop/restart/enable/disable control —
  the "beyond services.msc" equivalent.
- **Network tab** — every TCP/UDP(v4+v6) socket with owning process and state
  (`lsof`-grade, via `PROC_PIDFDSOCKETINFO`).
- **NetLog tab** — *real-time connection recorder* (goes beyond SI): every connection
  open / state-change / close is timestamped with the owning app, endpoints and
  lifetime, aggregated per application, with `Enter` jumping to the process.
  `--netlog <file>` streams the same events to disk as JSONL for forensics.
- **Per-app bandwidth** — live "which app is using the network right now" rates
  (Recv/s, Send/s) in the NetLog per-app table, sampled through Apple's own
  `nettop` on a background thread (kernel nstat accounting; approximate, external
  interfaces only, honestly labelled as such in the UI).
- **Events tab** — real-time system event stream: every process start (with its true
  start time, so sub-refresh processes are still attributed) and exit, plus launchd
  service state transitions. The entitlement-free approximation of an Endpoint
  Security exec/exit feed.
- **Sensors & battery** — SMC temperatures (CPU P/E-core, GPU, SSD) and fan rpm
  **without root**, plus battery percent/health/cycles/watts/time-remaining, shown in
  System Information (Ctrl+I). Root still adds powermetrics watts on top.
- **Threshold alerts (`a`)** — CPU / memory / disk-free / temperature / battery /
  process-gone rules with adjustable thresholds; raises once per condition, shows in
  the status bar and fires a macOS notification. Press `w` on any process to watch it.
- **Hostname resolution (`n`)** — Network and NetLog show `host:port` next to IPs via
  a cached, background (non-blocking) reverse-DNS resolver. SI's "Resolve hostnames".
- **Copy & export** — `c` copies the selected row (clipboard, TSV), `e` exports the
  whole current view to `~/Desktop/macinformer-<view>-<ts>.csv`.
- **Disk tab** — per-drive counters + live per-process disk activity.
- **System Information (Ctrl+I)** — scrolling graphs for CPU (incl. per-core P/E bars),
  Memory (incl. compressed/purgeable/swap), I/O, GPU (IOKit `IOAccelerator`),
  per-disk and per-interface, exactly like SI's System Information window.
- **Find Open Files (Ctrl+F)** — SI's "Find Handles or DLLs": system-wide descriptor
  search across files, sockets, pipes, kqueues with wildcard patterns.
- **CLI parity** — `--tab`, `--sysinfo`, `--selectpid`, `--refresh`, `--elevate`,
  plus `--netlog <file>` (JSONL connection-event recording).
- **`--server` (iOS companion / browser dashboard)** — headless agent mode: HTTP +
  RFC 6455 WebSocket server streaming the full live model (CPU/memory/disks/
  networks/GPU/sensors/battery/processes/connections/bandwidth/alerts + NetLog and
  Events feeds) as JSON. `GET /` serves a self-contained dark dashboard that any
  iPhone/iPad/Mac browser opens directly — the foundation the iOS app builds on.
  Loopback by default; `--listen 0.0.0.0:7270 --token SECRET` for LAN access.
- **Menu-bar app (`macinformer-menubar`)** — a proper macOS status-item shell:
  live CPU-history graph icon in the menu bar (template image, adapts to
  light/dark), dropdown with memory/GPU/sensors/battery/network numbers, top-5
  processes, alert state with notifications, "Open MacInformer TUI" and Quit.
  Same Rust core, still zero external crates (direct Cocoa FFI through libobjc).
- **AI assistant (`--ai`)** — ask questions about the live data and let the model
  *drive the app*. Providers: **Claude** (Anthropic), **Grok** (xAI), **Kimi**
  (Moonshot), and local **MLX** / **GGUF (llama.cpp)** / **ONNX** servers speaking
  the OpenAI protocol; `--ai auto` picks whatever is configured. Three surfaces:
  - **TUI chat view (key `i`)** — conversation with the live system state in every
    prompt; replies stream onto a worker thread so the UI never stalls. The model
    can emit audited actions — switch tabs, fill the search box, set alert
    thresholds, watch processes, export CSV, open SysInfo — executed with a toast
    so nothing happens silently.
  - **Dashboard chat drawer** — `--server --ai …` adds `POST /api/chat` and an AI
    drawer in the web dashboard; actions like "filter the process list to Safari"
    run client-side live. One shared conversation for every connected device.
  - **`--report <file.html>`** — standalone dark HTML report (CPU/memory/GPU/
    temperatures/battery cards, briefing, alerts, top processes, per-app bandwidth,
    disks, interfaces, connections, services). With `--ai` the briefing is written
    by the model; otherwise a rule-based briefing computed from the same real data
    is used and clearly labeled "built-in analysis".
  - HTTPS goes through the system `curl` (the OS TLS stack — same supported-tool
    philosophy as nettop/launchctl); plain `http://localhost` uses an internal
    client. API keys come from env vars (`ANTHROPIC_API_KEY`, `XAI_API_KEY`,
    `MOONSHOT_API_KEY`/`KIMI_API_KEY`) or `--ai-key`. Still zero crates: the JSON
    parser, SHA-1/base64, HTTP and WebSocket codecs are all hand-rolled.

The full, exact feature-by-feature mapping — including what macOS cannot do and why
(CPU affinity, closing foreign handles, per-process GPU) — is in
[docs/FEATURE_PARITY.md](docs/FEATURE_PARITY.md). Architecture, privilege model
(user → root → Endpoint Security helper) and the iOS companion design are in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Build & run (Mac mini M2)

```bash
# needs only: Xcode Command Line Tools + rustup (stable)
cargo build --release --target aarch64-apple-darwin
./target/aarch64-apple-darwin/release/macinformer            # full TUI
sudo ./target/aarch64-apple-darwin/release/macinformer       # full fidelity (I/O rates, FD tables, powermetrics)
./target/aarch64-apple-darwin/release/macinformer --sysinfo  # straight into graphs

# iOS companion / browser dashboard (then open http://<mac-ip>:7270/ in Safari)
./target/aarch64-apple-darwin/release/macinformer --server --listen 0.0.0.0:7270 --token pick-a-secret

# menu-bar app (live graph in the macOS menu bar)
./target/aarch64-apple-darwin/release/macinformer-menubar

# AI assistant: cloud (Claude shown; also grok / kimi) or local mlx / gguf / onnx
export ANTHROPIC_API_KEY=sk-ant-…
./target/aarch64-apple-darwin/release/macinformer --ai claude          # TUI, press i
./target/aarch64-apple-darwin/release/macinformer --ai gguf            # local llama.cpp on :8080
./target/aarch64-apple-darwin/release/macinformer --server --ai kimi   # dashboard chat drawer
./target/aarch64-apple-darwin/release/macinformer --report health.html --ai claude
```

## Keys

| Key | Action |
|---|---|
| `Ctrl+Q` | quit |
| `F1` / `F2` | help / platform capability notes |
| `F5` | refresh now |
| `Ctrl+I` | System Information graphs |
| `Ctrl+F` | Find Open Files (SI: Find Handles or DLLs) |
| `Ctrl+K` | search box (per tab) |
| `Tab`, `1–6` | switch tab (1 Processes, 2 Services, 3 Network, 4 NetLog, 5 Disk, 6 Events) |
| `←` `→` `r` | sort column / reverse |
| `Enter` | expand tree node / open process properties / jump to process from NetLog |
| `Space` | actions menu (terminate, suspend, nice, service control…) |
| `t` | tree / flat process list |
| `w` | watch/unwatch selected process (alert if it disappears) |
| `i` | AI assistant chat (requires `--ai …`) |
| `a` | threshold alerts dialog (toggle, adjust rules) |
| `n` | hostname resolution on/off |
| `c` / `e` | copy selected row / export view as CSV |
| `Esc` | close dialog / leave view / **quit from main screen** |
| `[` `]` | pages in Properties view |

## Verification status

- `cargo check --workspace` — clean (Linux)
- `cargo check --workspace --target aarch64-apple-darwin` — clean (all macOS FFI type-checked, incl. Cocoa menu-bar app and nettop sampler)
- `cargo test --workspace` — 87/87 passing (incl. end-to-end server tests: real HTTP + WebSocket handshake + streamed frames against a live listener; and AI tests: mock OpenAI server asserting request shape, full POST /api/chat round-trip through a live `--server`, action protocol parsing)
- `cargo clippy --workspace --all-targets` — clean on **both** Linux and aarch64-apple-darwin targets
- TUI exercised end-to-end in a real PTY against live data (screenshots above);
  both recorders validated against real traffic (curl HTTPS open/close captured with
  duration; process start/exit stream verified with live spawns)
- `--server` verified live: RFC 6455 handshake validated byte-for-byte, frames parsed
  and JSON-checked by an independent stdlib client, dashboard screenshot captured from
  a real headless browser consuming the stream
- AI verified live: TUI chat driven in a real PTY against a mock OpenAI server
  (question → reply → executed alert action visible in the status bar); dashboard
  chat driven over Chrome DevTools Protocol (question → reply → client-side filter
  action applied to the live process table); `--report` rendered and screenshotted
- Menu-bar app: Cocoa layer is compile-verified for aarch64-apple-darwin; its layout/
  formatting logic is platform-free and unit-tested. Runtime validation on the Mac
  itself is the one check that cannot run in CI (no macOS runner) — everything it
  displays comes from the same, already-tested core.

| AI chat in the TUI | AI chat in the dashboard | HTML report |
|---|---|---|
| ![chat](docs/screenshot-ai-chat.png) | ![dashchat](docs/screenshot-dashboard-chat.png) | ![report](docs/screenshot-report.png) |

## License

MIT. Not affiliated with winsiderss; "System Informer" names the upstream project this
ports. macOS, Mac mini and Apple Silicon are trademarks of Apple Inc.
