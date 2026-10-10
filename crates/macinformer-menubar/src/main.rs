//! macinformer-menubar — menu-bar GUI shell for MacInformer.
//!
//! macOS only. Lives in the menu bar as a live CPU graph; click for memory,
//! GPU, sensors, battery, network, top processes and alerts. Same Rust core
//! as the TUI (`macinformer-core`), zero external crates, direct Cocoa FFI.

// graph.rs is consumed by the macOS-only app layer; on other platforms only
// its unit tests reference it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod graph;

#[cfg(target_os = "macos")]
mod objc;

#[cfg(target_os = "macos")]
mod app;

#[cfg(target_os = "macos")]
fn main() {
    app::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macinformer-menubar is a macOS menu-bar app — build and run it on macOS (aarch64-apple-darwin).");
    eprintln!("On this platform use the `macinformer` TUI or `macinformer --server` instead.");
    std::process::exit(2);
}
