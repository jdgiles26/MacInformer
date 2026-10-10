# PRD — MacInformer

**Product**: MacInformer — a native macOS (Apple Silicon) re-implementation of Windows System Informer, written in Rust with zero external dependencies.
**Date**: 2026-10-11 · **Status**: Shipping (feature-complete per FEATURE_PARITY.md)

## 1. Purpose

Give macOS users the full System Informer capability set — process tree, system graphs, network connections, open handles, services, plus the MacInformer extras (NetLog recorder, alerts, AI assistant, headless server + dashboard, menu bar app) — as a single static binary with no runtime dependencies.

## 2. Functional Requirements

| ID | Requirement | Surface |
|---|---|---|
| F1 | Process list with tree view, CPU/mem/I/O columns, sort, search (substring + wildcard), per-process detail | TUI processes tab |
| F2 | Process actions: kill/signal, nice, properties (environment, open files, threads) | TUI actions modal |
| F3 | Live system information graphs: CPU (per-core + P/E split), memory, I/O, GPU, network, battery, sensors | TUI sysinfo view, dashboard |
| F4 | Network connections table with owning process, state, endpoint resolution | TUI network tab, `/api/snapshot` |
| F5 | Open files/handles system-wide (Find Handles equivalent) | TUI handles search |
| F6 | launchd services: list, start/stop/bootstrap actions | TUI services tab |
| F7 | NetLog: record connection lifecycle (open/state-change/close, duration), per-process aggregation, JSONL export | TUI netlog tab, `/api/netlog`, recorder.rs |
| F8 | System event recorder: process start/exit, service transitions, PID-reuse identity | sysrec.rs, `/api/events` |
| F9 | Alert rules: thresholds, watch process, quiet-when-healthy, desktop notification | alerts.rs, TUI alerts modal |
| F10 | AI assistant: provider abstraction (Claude, Grok, Kimi, MLX, GGUF/llama.cpp, ONNX local), chat with action extraction, computer-use suggestions | TUI chat (`i`), dashboard chat, `/api/chat` |
| F11 | Headless server: HTTP dashboard + REST + WebSocket stream, optional token gate | `--server`, server.rs |
| F12 | Menu bar app: live graphs, quick actions, launch TUI | macinformer-menubar |
| F13 | One-shot report: Markdown + HTML with narrative analysis | `--report`, report.rs |
| F14 | Linux fallback platform: full UI functionality via /proc for CI/testing | platform/linux.rs |
| F15 | Non-TTY operation: `--dump WxH` frame render; TUI refuses non-TTY stdin with a clean error | term.rs, `--dump` |

## 3. Non-Functional Requirements

| ID | Requirement | Envelope |
|---|---|---|
| N1 | Zero external dependencies — pure std + FFI | `Cargo.lock` contains only the 4 workspace crates |
| N2 | Memory safety | every `unsafe` block carries a `// SAFETY:` invariant comment |
| N3 | No panics in production paths | 0 `panic!`/`unreachable!`; unwrap only where provably infallible or startup-fatal |
| N4 | DoS resistance on the server | 1 MB body cap, 16 KB header cap, 10 s read deadline, JSON depth cap 128 |
| N5 | Refresh cadence | default 1 s tick; TUI frame render < 16 ms on M2 |
| N6 | Binary footprint | single release binary per crate; no runtime assets (dashboard embedded) |
| N7 | Portability | primary `aarch64-apple-darwin`; CI fallback `x86_64-unknown-linux-gnu`; clippy clean on both |
| N8 | Formatting/lint hygiene | `cargo fmt --check` and `cargo clippy -D warnings` are merge gates |

## 4. Zero-Trust Execution Criteria

| ID | Criterion |
|---|---|
| Z1 | No telemetry, no beacons, no external network calls except the operator-configured LLM endpoint |
| Z2 | Credentials never in source, never in logs, never in the repo; resolved at runtime from CLI arg → env → macOS Keychain |
| Z3 | Server binds 127.0.0.1 by default; LAN bind requires explicit `--listen 0.0.0.0:PORT`; token gate available for all `/api/*` and `/ws` |
| Z4 | All child processes spawned with absolute paths and argv arrays — no shell anywhere |
| Z5 | All HTML rendering of dynamic strings escaped (`esc()`) or assigned via `textContent` |
| Z6 | Every external input (HTTP request, /proc file, IOKit struct, nettop CSV) parsed defensively with bounds checks |
| Z7 | No mocks, stubs, placeholders, or hidden functionality in shipped code (mocks permitted in tests only) |

## 5. Out of Scope

- iOS app (the dashboard + server are the iOS companion surface via browser)
- Kernel extensions / private frameworks requiring entitlements beyond standard IOKit
- Windows/Linux as primary targets (Linux exists only as the CI fallback collector)
