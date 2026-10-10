# MacInformer — Architecture

A macOS-native port of System Informer's feature set, written in **Rust** (zero external
dependencies — every syscall is direct FFI, mirroring System Informer's own dependency-light C design).
Reference hardware: Mac mini M2 (`aarch64-apple-darwin`, macOS 13+).

## 1. High-level shape

```
┌─────────────────────────────── user space ───────────────────────────────┐
│  macinformer (TUI)        macinformer-menubar (menu-bar GUI, this repo)  │
│  ┌─────────────────┐      ┌────────────────────────────────────────────┐ │
│  │ macinformer-tui │      │ NSStatusItem graphs + menu (Cocoa FFI)     │ │
│  └───────┬─────────┘      └───────────────┬────────────────────────────┘ │
│          │            shared Rust core     │                              │
│  ┌───────▼─────────────────────────────────▼──────────────────────────┐  │
│  │                     macinformer-core (lib)                          │  │
│  │  processes │ cpu │ memory │ disks │ network │ connections │ gpu │   │  │
│  │  services(launchd) │ openfiles │ thermal │ detail │ actions        │  │
│  │  recorder │ sysrec │ sensors │ alerts │ bandwidth │ server(ws/http)│  │
│  │  json(parse+write) │ llm(providers/actions) │ report(html)        │  │
│  └───────┬───────────────────────────────────────────────────────────┘  │
│          │ XPC (SMJobBless) — only when elevated features are used      │
│  ┌───────▼─────────┐    ┌──────────────────────┐                        │
│  │ PrivilegedHelper │───▶│ Endpoint Security ext │ (optional, entitled) │
│  │ (root, launchd)  │    └──────────────────────┘                        │
│  └─────────────────┘                                                   │
└─────────────────────────────────────────────────────────────────────────┘
 iOS companion app / any browser ──WebSocket──▶ macinformer --server (shipped)
```

## 2. Crate layout (this repository)

```
MacInformer/
├── Cargo.toml                 # workspace
├── src/main.rs                # `macinformer` binary: CLI, flags, TUI/server wiring
├── crates/
│   ├── macinformer-core/      # data model + collectors + server + actions (zero deps)
│   │   └── src/platform/
│   │       ├── macos/         # real collectors: libproc/Mach/IOKit/launchd FFI
│   │       ├── linux/         # /proc fallback — lets the app run & be tested anywhere
│   │       └── stub.rs        # everything else
│   ├── macinformer-tui/       # hand-rolled ANSI terminal UI (zero deps)
│   └── macinformer-menubar/   # macOS menu-bar GUI shell (direct Cocoa FFI, zero deps)
└── docs/
```

- **Zero external crates.** `cargo build --target aarch64-apple-darwin` works offline.
- The Linux fallback is not a toy — it exercises 100% of the UI against live data and is
  how the project is CI-tested without a Mac.

## 3. Data sources (macOS)

| Subsystem | Source (all public, stable ABI) |
|---|---|
| Process list | `proc_listpids` + `proc_pidinfo(PROC_PIDTBSDINFO / PROC_PIDTASKINFO)` |
| Process path/cmdline/env | `proc_pidinfo(PROC_PIDVNODEPATHINFO)`, `sysctl kern.procargs2` |
| Per-proc I/O bytes | `proc_pid_rusage(RUSAGE_INFO_V4)` → `ri_diskio_bytesread/written` |
| CPU (total + per-core, P/E) | `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`, `hw.perflevel*` |
| Memory lists + swap | `host_statistics64(HOST_VM_INFO64)`, `sysctl vm.swapusage` |
| Disk capacity/mounts | `getmntinfo` + `statfs64` |
| Disk I/O counters | IOKit `IOBlockStorageDriver` → `Statistics` properties |
| NIC byte counters | `getifaddrs(AF_LINK)` + `if_data64` |
| Connections table | FD walk: `proc_pidinfo(PROC_PIDLISTFDS)` + `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` (lsof's method) |
| Connection recording (NetLog) | per-tick diff of the connections table → open/state/close events + per-app stats (`recorder.rs`); JSONL export via `--netlog` |
| Per-app bandwidth | `nettop -P -x -J bytes_in,bytes_out` on a background sampler thread (`bandwidth.rs`); cumulative counters → rates from consecutive samples; approximate, external interfaces only |
| System event stream (Events) | per-tick process/service snapshot diff → start/exit/transition events (`sysrec.rs`) |
| Temperatures & fan (no root) | AppleSMC IOKit user client, selector 2 key reads (`sensors.rs`), M1–M4 key lists |
| Battery / power source | `IOPSCopyPowerSourcesInfo` (no root) |
| Hostname resolution | background reverse-DNS worker w/ pos/neg caching (`resolver.rs`), UI never blocks |
| Threshold alerts | pure rule evaluation (`alerts.rs`) + osascript notification delivery |
| Open files (Ctrl+F) | same FD walk, `PROC_PIDFDVNODEPATHINFO` |
| GPU | IOKit `IOAccelerator` → `PerformanceStatistics` (Device Utilization %, memory) |
| Thermal/power | `powermetrics --samplers thermal,cpu_power -n1` when root (optional) |
| launchd services | `launchctl print-cache`/`list` + plist scan of `/Library/Launch*` |
| Service control | `launchctl bootstrap/bootout/kickstart/kill` |
| Code-signing flags | `codesign -dv` (cached) |
| Event log | process diff + `kqueue NOTE_EXIT` for watched pids + IOKit notifications |

## 4. Privilege model

Three tiers, resolved at runtime (mirrors SI's normal → elevate → KPH driver tiers):

1. **User** — everything in §3 except fs_usage/powermetrics/foreign task ports. Fully usable.
2. **Root** (`sudo macinformer`, or `--elevate` re-exec) — adds fs_usage disk tab events,
   powermetrics thermals, renice below 0, signals to other users' processes.
3. **Privileged helper + Endpoint Security** (optional install) — `task_for_pid` introspection,
   real-time AUTH/NOTIFY event stream. Entitlement must be granted by Apple per developer ID;
   the code path is behind `platform/macos/es/` and the app degrades gracefully without it.

## 5. UI

- **TUI (shipped here):** replicates the main window 1:1 — menu bar, toolbar, tab strip
  (Processes · Services · Network · NetLog · Disk · Events), searchable/sortable tables,
  process tree with expand/collapse, color highlighting, status bar (CPU, physical, free),
  System Information graphs view (`Ctrl+I`), Find Open Files (`Ctrl+F`), process properties
  (`Enter`), actions menu (`Space`), help (`F1`). Beyond SI parity: NetLog connection
  recorder with JSONL export, Events stream, SMC sensors/battery panel, threshold alerts
  with macOS notifications, reverse-DNS, CSV export and clipboard copy.
- **GUI menu-bar shell (shipped, same core):** `macinformer-menubar` — an
  NSStatusItem app built with direct libobjc/AppKit FFI (no crates): live
  CPU-history graph icon (template image), dropdown with memory/GPU/sensors/
  battery/network numbers, top-5 processes, alert state + notifications,
  "Open MacInformer TUI", Quit. All presentation logic is platform-free
  (`graph.rs`) and unit-tested; the Cocoa layer is compile-verified for
  aarch64-apple-darwin and needs one runtime pass on a real Mac (CI has no
  macOS runner).
- **iOS companion (server shipped):** `macinformer --server` (`server.rs` in core)
  is a dependency-free HTTP + RFC 6455 WebSocket server (hand-rolled SHA-1,
  base64, frame codec) streaming the full live model as JSON: snapshots,
  connections, per-app bandwidth, sensors/battery, alerts, NetLog + Events feeds.
  `GET /` serves a self-contained dashboard that renders the stream in any
  browser — the reference client for the future native iOS app, and already
  usable from an iPhone on the same LAN (`--listen 0.0.0.0:7270 --token …`).
- **AI assistant (shipped):** `llm.rs` in core — provider presets for Claude,
  Grok, Kimi (cloud, HTTPS via the system curl = the OS TLS stack) and local
  MLX / llama.cpp GGUF / ONNX servers (OpenAI protocol over plain HTTP via an
  internal client). `--ai auto` detects env keys and local servers. Every turn
  ships a fresh `context_digest` (the same one for TUI, dashboard and report)
  plus an audited action vocabulary: replies may carry `ACTION: {json}` lines
  which the TUI executes (switch tab, search, set alerts, watch process,
  export CSV) and the dashboard applies client-side (live table filters).
  Surfaces: TUI chat view (`i`, worker thread + mpsc so the UI never blocks),
  `POST /api/chat` + dashboard drawer (one shared conversation per server),
  and `--report` (`report.rs`): a standalone HTML report whose briefing is
  model-written when a provider is set, or a rule-based narrative computed
  from the same real data and labeled as such.

## 6. Portability & build

```bash
# On the Mac mini M2 (Xcode CLT only, nothing else):
cargo build --release --target aarch64-apple-darwin
# Result: single ~4 MB binary, no install, portable mode via macinformer.toml sidecar.
```

The codebase is `no_std`-free but dependency-free; MSRV = stable 1.80+.
Verified in CI-equivalent form: `cargo check --target aarch64-apple-darwin` +
`cargo test` + `cargo clippy` on Linux (all macOS-only code is `cfg`-gated and type-checked).

## 7. What this is not

- Not a wrapper around `ps`/`top`/`lsof` — those are used nowhere in the hot path
  (except `launchctl` for service control, which *is* the supported interface).
- Not jailbreak tooling: ⛔ items in the parity matrix are platform absences, documented,
  with the closest supported substitute wired in.
