//! Platform dispatch: real collectors on macOS, a fully working /proc-based
//! fallback on Linux (used for CI/testing of the entire UI), stubs elsewhere.

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub mod stub;

pub fn new_collector() -> Box<dyn crate::Collector + Send> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacCollector::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxCollector::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(stub::StubCollector)
    }
}
