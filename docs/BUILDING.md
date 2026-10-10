# Building MacInformer on a Mac mini M2

## Prerequisites

```bash
xcode-select --install                      # Command Line Tools (linker + SDK)
curl https://sh.rustup.rs -sSf | sh         # Rust stable
rustup target add aarch64-apple-darwin
```

No other dependencies. The project has **zero crates.io dependencies** — it builds offline.

## Build

```bash
git clone <repo> && cd MacInformer
cargo build --release --target aarch64-apple-darwin
install -m 755 target/aarch64-apple-darwin/release/macinformer /usr/local/bin/
```

Single static binary (~4 MB). Portable mode: drop a `macinformer.toml` next to the binary.

## Privilege tiers

| Tier | How | What unlocks |
|---|---|---|
| User | `./macinformer` | processes, CPU/mem, disks, NICs, GPU, connections, services (read) |
| Root | `sudo ./macinformer` or `./macinformer --elevate` | + all users' FD tables & I/O rates, powermetrics GPU power/temps, renice down, signals to any process, service control |
| Entitled | privileged helper (roadmap, `com.apple.developer.endpoint-security.client`) | + real-time event stream, task_for_pid introspection of non-SIP processes |

SIP platform binaries (`/System/*`) cannot be attached even as root — Apple design, see
`docs/FEATURE_PARITY.md` §11.

## Verifying without a Mac

```bash
cargo check --workspace --target aarch64-apple-darwin   # type-checks all macOS FFI
cargo test --workspace                                   # 20 unit tests (incl. parsers)
cargo run -- --dump                                      # static frame via the Linux fallback collector
```

## iOS companion (design)

`macinformer --server` (roadmap module) streams snapshots over WebSocket; the iOS app
renders the same tabs. Standalone iOS process enumeration is impossible under App Store
sandboxing — see FEATURE_PARITY §12.
