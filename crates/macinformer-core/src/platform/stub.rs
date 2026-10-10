//! Stub collector for unsupported platforms.

use crate::types::*;

pub struct StubCollector;

impl crate::Collector for StubCollector {
    fn snapshot(&mut self) -> Snapshot {
        Snapshot::default()
    }
    fn connections(&mut self) -> Vec<Connection> {
        Vec::new()
    }
    fn open_files(&mut self) -> Vec<OpenFile> {
        Vec::new()
    }
    fn services(&mut self) -> Vec<ServiceInfo> {
        Vec::new()
    }
    fn environment(&self, _pid: i32) -> Vec<(String, String)> {
        Vec::new()
    }
    fn process_detail(&mut self, _pid: i32) -> Option<ProcessDetail> {
        None
    }
    fn send_signal(&self, _pid: i32, _sig: i32) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    fn set_nice(&self, _pid: i32, _nice: i32) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    fn service_action(&self, _label: &str, _action: ServiceAction) -> Result<String, String> {
        Err("unsupported platform".into())
    }
    fn capability_notes(&self) -> Vec<String> {
        vec!["Unsupported platform — macOS (aarch64-apple-darwin) is the target.".into()]
    }
    fn is_root(&self) -> bool {
        false
    }
}
