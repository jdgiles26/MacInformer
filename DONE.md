# DONE.md — Acceptance Ledger

Binary-verifiable acceptance criteria. Each item maps to executable proof (test assertion, gate command, or scripted check). Status: **15/15 PASS** as of 2026-10-11.

| # | Criterion | Proof (test assertion / command) | Status |
|---|---|---|---|
| 1 | Workspace builds clean on host and `aarch64-apple-darwin` | `cargo check --workspace` + `cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings` | PASS |
| 2 | Zero external dependencies | `Cargo.lock` lists exactly 4 packages (the workspace crates); `cargo tree` shows no externals | PASS |
| 3 | JSON parser round-trips nested values, escapes, surrogate pairs | `json::tests::parse_roundtrip_nested`, `escapes_control_and_quotes`, `parse_surrogate_pair` | PASS |
| 4 | JSON parser rejects hostile nesting without crashing | `json::tests::deep_nesting_is_rejected_not_crashing` (depth 138 errors, 126 parses); live: 400 K `[` POST → clean error, server survives | PASS |
| 5 | HTTP server parses requests, enforces method + token gates | `server::tests::http_request_parsing`; integration `token_gate_enforced`, `dashboard_and_rest_endpoints` | PASS |
| 6 | WebSocket handshake conforms to RFC 6455 and streams frames | `server::tests::ws_accept_rfc_example`, `sha1_known_vectors`, `base64_vectors`, `frame_encoding_lengths`; integration `websocket_handshake_and_stream` | PASS |
| 7 | Chat endpoint works end-to-end and degrades gracefully | `server_chat_endpoint_full_stack`; `server_chat_without_provider_returns_error_not_crash` | PASS |
| 8 | LLM providers resolve and serialize correct request bodies (OpenAI + Claude shapes), parse replies, extract actions, ignore fenced/bad JSON | `llm::tests::resolves_providers_and_keys`, `openai_body_shape`, `claude_body_and_response`, `parses_actions_out_of_reply`, `ignores_actions_inside_code_fences_and_bad_json`; integration `assistant_roundtrip_against_mock_openai` | PASS |
| 9 | NetLog recorder captures open/state-change/close with duration, aggregates per process, respects caps, emits valid JSONL | `recorder::tests::records_open_then_close`, `records_state_transitions`, `aggregates_per_process`, `respects_event_cap`, `json_is_valid_and_escaped` | PASS |
| 10 | System event recorder tracks start/exit/service transitions and survives PID reuse | `sysrec::tests::start_and_exit_recorded`, `service_transitions_recorded`, `pid_reuse_keeps_last_seen_identity`, `steady_state_is_quiet` | PASS |
| 11 | Alerts fire per rule, fire once then clear, respect watch/quiet modes | `alerts::tests::fires_each_rule`, `tracker_fires_once_then_clears`, `quiet_when_healthy`, `watched_process_present_is_quiet`, `disabled_is_silent` | PASS |
| 12 | TUI primitives correct: buffer diffing, key parsing (UTF-8/arrows/F-keys/Ctrl), table sort/tree, graph scaling/clamping | `buffer::tests::*`, `input::tests::*` (4), `widgets::tests::*`, `graph::tests::*` (6) | PASS |
| 13 | Live collector produces a real snapshot on the CI platform | `platform::linux::tests::live_snapshot_runs`, `stat_parse`, `addr_parse_v4`; live run: 312 processes observed | PASS |
| 14 | Reports cite real numbers, render Markdown→HTML with escaping | `report::tests::narrative_cites_real_numbers_and_alerts`, `markdown_renders_and_escapes`, `narrative_healthy_system_says_so` | PASS |
| 15 | Dashboard loads with a clean console and escaped rendering | headless Chromium: DOM renders, 0 console errors/deprecations; grep-verified `esc()` on all innerHTML-bound strings; zero `console.*` | PASS |

## Gate commands (merge blockers)

```sh
cargo fmt --check                                            # formatting
cargo clippy --workspace --all-targets -- -D warnings        # lints (host)
cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings
cargo test --workspace                                       # 88 tests
cargo build --release --workspace                            # release
tools/drift.sh                                               # runtime envelope check (needs a free port)
```

Any criterion flipping to FAIL blocks push until fixed or the criterion is amended here with a written reason.
