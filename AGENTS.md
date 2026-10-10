# AGENTS.md — Workflow Roles & Tool Contracts

How automated agents (and humans) are expected to work on this repository.

## Roles

| Role | Responsibility | Boundaries |
|---|---|---|
| **Implementer** | Writes/changes code to satisfy DONE.md criteria | Must not weaken a gate, delete a test to make it pass, or add an external dependency |
| **Reviewer (rust)** | Runs the rust-review rubric (`docs/reviews/rust-review.md` format): gates first, then unsafe/ownership/error-handling review | Findings need a file:line and a nameable failure mode |
| **Reviewer (adversarial)** | Independent second pass per `santa-loop` rubric; must not see Reviewer A's output | No code changes — verdict JSON only |
| **Gatekeeper** | Runs DONE.md gate commands; only pushes when 15/15 PASS | Push requires santa-loop NICE |
| **Drift monitor** | Runs `tools/drift.sh` against a live instance | Read-only; never mutates the tree |

## Standing constraints (apply to every role)

1. **Zero external dependencies.** No crates.io additions, no npm, no CDNs, no embedded third-party assets. New capability = new FFI binding or new std code.
2. **No mocks/stubs/placeholders in shipped code.** Mocks exist only under `tests/`.
3. **Every `unsafe` block carries a `// SAFETY:` invariant** stating pointer provenance, ownership, and ABI assumptions.
4. **Errors are values**: `Result<T, String>` (core) / `io::Result` (TUI) — no `panic!`, no silent swallow (`let _ =` only on best-effort side effects).
5. **Secrets**: never in source, never logged. Resolution order: `--ai-key` → `MACINFORMER_AI_KEY` env → macOS Keychain.
6. **Shell-out policy**: absolute paths + argv arrays only; never interpolate untrusted input; escape when the child is an interpreter (osascript).
7. **HTML safety**: dynamic strings go through `esc()` before innerHTML, or use `textContent`.

## Tool contracts

This repo is toolchain-pure; the only external tools are the Rust toolchain and (for drift) curl.

| Tool | Invocation | Contract |
|---|---|---|
| cargo fmt | `cargo fmt --check` | exit 0 required |
| cargo clippy | `cargo clippy --workspace --all-targets -- -D warnings` (+ `--target aarch64-apple-darwin`) | exit 0 required, both targets |
| cargo test | `cargo test --workspace` | 88/88 required |
| cargo build | `cargo build --release --workspace` | exit 0 required |
| drift | `tools/drift.sh [port]` | exit 0; non-zero = operational envelope violated |
| MCP `push_files` (publish) | batched file push to `jdgiles26/MacInformer` | only after NICE verdict; binary assets (PNG) stay local |

## Adding a feature (required sequence)

1. Add/adjust the criterion in DONE.md first (spec before code).
2. Write the failing test asserting the criterion.
3. Implement until green; keep functions focused and dispatch tables flat.
4. Run all gate commands + `tools/drift.sh`.
5. Update FEATURE_PARITY.md if the feature changes parity status.
6. Push only via the Gatekeeper path (santa-loop NICE + gates green).
