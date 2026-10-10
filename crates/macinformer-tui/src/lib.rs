//! macinformer-tui — a hand-rolled ANSI terminal UI replicating the
//! System Informer main window: menu bar, toolbar, tabs, sortable/searchable
//! tables with tree view and color highlighting, status bar, System
//! Information graphs, Find Open Files, and process properties.
//!
//! Zero external dependencies (raw termios + ANSI escape codes).

pub mod app;
pub mod chat;
pub mod buffer;
pub mod input;
pub mod term;
pub mod widgets;

pub use app::{App, AppConfig};
