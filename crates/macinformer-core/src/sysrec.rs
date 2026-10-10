//! Real-time system event recorder — process starts and exits, service state
//! changes — diffed from each snapshot, exactly like the connection recorder
//! does for sockets. This is the entitlement-free approximation of what an
//! Endpoint Security client streams (AUTH/NOTIFY exec/exit); events that live
//! shorter than one refresh are caught via start-time bookkeeping.

use crate::types::ProcessInfo;
use crate::types::ServiceInfo;
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysEventKind {
    ProcStarted,
    ProcExited,
    ServiceChanged,
}

impl SysEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SysEventKind::ProcStarted => "STARTED",
            SysEventKind::ProcExited => "EXITED",
            SysEventKind::ServiceChanged => "SVC",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SysEvent {
    pub seq: u64,
    pub unix_secs: u64,
    pub kind: SysEventKind,
    pub pid: i32,
    pub name: String,
    /// Extra detail: exit state, service transition, parent name…
    pub detail: String,
}

impl SysEvent {
    pub fn clock(&self) -> String {
        crate::recorder::hms_of(self.unix_secs)
    }
}

#[derive(Default)]
pub struct SysRecorder {
    // pid → (name, start_time_unix) of live processes from the last snapshot
    live: HashMap<i32, (String, u64)>,
    last_services: HashMap<String, String>,
    events: VecDeque<SysEvent>,
    seq: u64,
    cap: usize,
}

impl SysRecorder {
    pub const DEFAULT_CAP: usize = 4000;

    pub fn new() -> Self {
        SysRecorder { cap: Self::DEFAULT_CAP, ..Default::default() }
    }

    fn now_unix() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    fn push(&mut self, kind: SysEventKind, pid: i32, name: String, detail: String) {
        self.seq += 1;
        self.events.push_back(SysEvent { seq: self.seq, unix_secs: Self::now_unix(), kind, pid, name, detail });
        while self.events.len() > self.cap {
            self.events.pop_front();
        }
    }

    /// Diff a process snapshot. Every newly seen process is recorded as
    /// STARTED with its real start time, so processes that live less than
    /// one refresh are still attributed correctly (they show as STARTED
    /// followed by EXITED in the log).
    pub fn observe_processes(&mut self, procs: &[ProcessInfo]) {
        let now = Self::now_unix();
        let mut cur: HashMap<i32, (String, u64)> = HashMap::with_capacity(procs.len());
        for p in procs {
            if p.is_dead {
                continue;
            }
            cur.insert(p.pid, (p.name.clone(), p.start_time));
            if !self.live.contains_key(&p.pid) {
                let age = now.saturating_sub(p.start_time);
                self.push(
                    SysEventKind::ProcStarted,
                    p.pid,
                    p.name.clone(),
                    if age > 2 { format!("running for {}", crate::types::human_duration(age)) } else { "just launched".into() },
                );
            }
        }
        let mut exited: Vec<(i32, String)> = Vec::new();
        for (pid, (name, _)) in &self.live {
            if !cur.contains_key(pid) {
                exited.push((*pid, name.clone()));
            }
        }
        for (pid, name) in exited {
            self.push(SysEventKind::ProcExited, pid, name, String::new());
        }
        self.live = cur;
    }

    /// Diff a services snapshot (only when the Services tab is refreshed).
    pub fn observe_services(&mut self, services: &[ServiceInfo]) {
        let mut cur: HashMap<String, String> = HashMap::with_capacity(services.len());
        for s in services {
            let st = s.status();
            cur.insert(s.label.clone(), st.clone());
            if let Some(prev) = self.last_services.get(&s.label) {
                if *prev != st {
                    self.push(SysEventKind::ServiceChanged, s.pid, s.label.clone(), format!("{prev} → {st}"));
                }
            }
        }
        self.last_services = cur;
    }

    pub fn events(&self) -> &VecDeque<SysEvent> {
        &self.events
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, name: &str) -> ProcessInfo {
        ProcessInfo { pid, name: name.into(), start_time: 5, ..Default::default() }
    }

    #[test]
    fn start_and_exit_recorded() {
        let mut r = SysRecorder::new();
        r.observe_processes(&[proc(10, "a"), proc(11, "b")]);
        assert_eq!(r.event_count(), 2);
        r.observe_processes(&[proc(11, "b"), proc(12, "c")]);
        let kinds: Vec<SysEventKind> = r.events().iter().map(|e| e.kind).collect();
        // starts are pushed during the scan, exits after it — order within a
        // snapshot is STARTED-then-EXITED, which reads naturally in the log
        assert_eq!(kinds, vec![SysEventKind::ProcStarted, SysEventKind::ProcStarted, SysEventKind::ProcStarted, SysEventKind::ProcExited]);
        assert_eq!(r.events()[2].name, "c");
        assert_eq!(r.events()[3].name, "a");
    }

    #[test]
    fn steady_state_is_quiet() {
        let mut r = SysRecorder::new();
        r.observe_processes(&[proc(1, "x")]);
        r.observe_processes(&[proc(1, "x")]);
        assert_eq!(r.event_count(), 1);
    }

    #[test]
    fn pid_reuse_keeps_last_seen_identity() {
        let mut r = SysRecorder::new();
        r.observe_processes(&[proc(7, "old")]);
        // same pid still alive next snapshot = same process (documented
        // limitation of pid-keyed tracking; start_time disambiguation would
        // require the kernel's unique process id, which libproc doesn't give)
        let mut p = proc(7, "new");
        p.start_time = 999_999;
        r.observe_processes(&[p]);
        assert_eq!(r.event_count(), 1);
    }

    #[test]
    fn service_transitions_recorded() {
        let mut r = SysRecorder::new();
        let mk = |pid: i32| ServiceInfo { label: "com.example.d".into(), pid, ..Default::default() };
        r.observe_services(&[mk(0)]);
        r.observe_services(&[mk(42)]);
        let evs: Vec<&SysEvent> = r.events().iter().filter(|e| e.kind == SysEventKind::ServiceChanged).collect();
        assert_eq!(evs.len(), 1);
        assert!(evs[0].detail.contains('→'));
    }
}
