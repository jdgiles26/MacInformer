# CLAUDE.md — Instructions for LLM-Assisted Edits

You are editing **MacInformer**: a zero-dependency Rust re-implementation of Windows System Informer for macOS/Apple Silicon. Read `PRD.md`, `DONE.md`, and `AGENTS.md` before changing anything.

## Architecture in one paragraph

Four crates, no external dependencies. `macinformer-core` owns all system access (Mach/IOKit/proc-libproc FFI on macOS under `platform/macos/`, a fully working `/proc` fallback under `platform/linux/`, stub elsewhere), plus json/http/server/llm/recorder/alerts/report. `macinformer-tui` is the terminal UI (own termios layer, diffing buffer, widgets). `macinformer-menubar` is the Cocoa menu bar app (own objc_msgSend bindings). The root `macinformer` binary parses CLI args and dispatches to TUI / server / report / dump modes. The web dashboard is a single embedded HTML file (`core/src/dashboard.html`) served by `server.rs`.

## Non-negotiable rules

1. **Zero external dependencies.** Cargo.toml dependency sections stay empty. Forever. If a feature seems to need a crate, write the FFI binding instead (see `platform/macos/ffi.rs`, `menubar/objc.rs` for the established pattern).
2. **Every `unsafe` block gets a `// SAFETY:` comment** stating the invariant (pointer provenance, buffer ownership/length, struct layout, thread confinement). No exceptions — the review gate counts them 1:1.
3. **No panics in production paths.** `unwrap` only where provably infallible (fixed-size `try_into`, NUL-free literal `CString::new`) or startup-fatal with a message. Everything else returns `Result`.
4. **Recursive parsers must be depth-capped** (see `json.rs::MAX_DEPTH`). Any new parser for untrusted input needs the same treatment and a regression test.
5. **Child processes**: absolute path + argv array, never a shell string. Escape `\` and `"` before interpolating into AppleScript.
6. **dashboard.html**: every dynamic string passes `esc()` before innerHTML; chat content uses `textContent`. No external URLs, no `console.*`.
7. **Server inputs are hostile**: keep the 1 MB body cap, 16 KB header cap, 10 s deadline, and token gate semantics intact when editing `server.rs`.
8. **Mocks only under `tests/`.** Production code never contains test doubles, placeholders, or feature flags that silently degrade.

## Workflow

- Spec first: add the acceptance criterion to `DONE.md`, then the failing test, then the code.
- Gates before every commit: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` on host **and** `--target aarch64-apple-darwin`, `cargo test --workspace`, `cargo build --release --workspace`, `tools/drift.sh`.
- Keep render/dispatch functions flat and linear; don't split them purely for line count (see the disposition note in `docs/reviews/rust-review.md`).
- Update `docs/FEATURE_PARITY.md` when parity-relevant behavior changes.

## Known accepted trade-offs (do not "fix" without discussion)

- `POST /api/chat` returns HTTP 200 with an `{"error": ...}` body for validation failures (the embedded dashboard reads `.error`). Recorded as LOW in the review ledger.
- Token comparison is plain string equality and tokens travel as `?token=` query params — acceptable for a localhost monitoring tool; harden only if the threat model changes.
- Thread-per-connection HTTP server without a connection cap — localhost-first design.
- macOS-only features (SMC sensors, nettop bandwidth, launchd, Cocoa) are `cfg`-gated; the Linux fallback must keep compiling — that's what CI exercises.
