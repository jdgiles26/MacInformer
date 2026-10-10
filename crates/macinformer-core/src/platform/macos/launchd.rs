//! launchd services — the macOS counterpart of SI's Services tab
//! ("Go beyond services.msc": list, inspect, create/edit, control).

use crate::types::*;
use std::collections::HashMap;
use std::path::PathBuf;

const LAUNCH_DIRS_SYSTEM: [&str; 2] = ["/Library/LaunchDaemons", "/System/Library/LaunchDaemons"];
const LAUNCH_DIRS_AGENT: [&str; 2] = ["/Library/LaunchAgents", "/System/Library/LaunchAgents"];

/// `launchctl list` → label → (pid, last_exit_status).
fn launchctl_list() -> HashMap<String, (i32, i64)> {
    let mut map = HashMap::new();
    if let Ok(o) = std::process::Command::new("/bin/launchctl").arg("list").output() {
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines().skip(1) {
            let mut f = line.split('\t');
            let (Some(pid_s), Some(status_s), Some(label)) = (f.next(), f.next(), f.next()) else { continue };
            let pid = pid_s.trim().parse().unwrap_or(0);
            let status = status_s.trim().parse().unwrap_or(0);
            map.insert(label.trim().to_string(), (pid, status));
        }
    }
    map
}

/// Disabled labels from `launchctl print-disabled` (best-effort, per-domain).
fn disabled_labels() -> HashMap<String, bool> {
    let mut map = HashMap::new();
    let uid = unsafe { super::ffi::geteuid() };
    for domain in ["system".to_string(), format!("gui/{uid}")] {
        if let Ok(o) = std::process::Command::new("/bin/launchctl").args(["print-disabled", &domain]).output() {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                // lines look like:  "com.example.foo" => disabled
                if let Some((label, state)) = line.split_once("=>") {
                    let label = label.trim().trim_matches('"');
                    map.insert(label.to_string(), state.contains("disabled") && !state.contains("enabled"));
                }
            }
        }
    }
    map
}

// ------------------------------------------------- minimal plist parser

#[derive(Default, Debug, PartialEq)]
pub struct PlistSummary {
    pub label: String,
    pub program: String,
    pub run_at_load: bool,
    pub keep_alive: bool,
    pub disabled: bool,
}

/// Extract the few keys we need from an XML plist without a full parser.
pub fn parse_plist(xml: &str) -> PlistSummary {
    let mut s = PlistSummary::default();
    let mut chars = xml;
    while let Some(kpos) = chars.find("<key>") {
        let rest = &chars[kpos + 5..];
        let Some(kend) = rest.find("</key>") else { break };
        let key = rest[..kend].trim().to_string();
        let after = &rest[kend + 6..];
        let (val, consumed) = read_value(after);
        match key.as_str() {
            "Label" => s.label = val,
            "Program" => s.program = val,
            "ProgramArguments" => {
                if s.program.is_empty() {
                    s.program = val;
                }
            }
            "RunAtLoad" => s.run_at_load = val == "true",
            "KeepAlive" => s.keep_alive = val == "true",
            "Disabled" => s.disabled = val == "true",
            _ => {}
        }
        chars = &after[consumed.min(after.len())..];
    }
    s
}

/// Read the value node following a <key>. Returns (stringified value, bytes consumed).
fn read_value(s: &str) -> (String, usize) {
    let t = s.trim_start();
    let lead = s.len() - t.len();
    if let Some(rest) = t.strip_prefix("<string>") {
        if let Some(e) = rest.find("</string>") {
            return (rest[..e].trim().to_string(), lead + 8 + e + 9);
        }
    } else if t.starts_with("<true/>") {
        return ("true".into(), lead + 7);
    } else if t.starts_with("<false/>") {
        return ("false".into(), lead + 8);
    } else if let Some(rest) = t.strip_prefix("<integer>") {
        if let Some(e) = rest.find("</integer>") {
            return (rest[..e].trim().to_string(), lead + 9 + e + 10);
        }
    } else if let Some(rest) = t.strip_prefix("<array>") {
        // first <string> inside the array (ProgramArguments)
        if let Some(e) = rest.find("</array>") {
            let body = &rest[..e];
            if let Some(sp) = body.find("<string>") {
                if let Some(se) = body[sp + 8..].find("</string>") {
                    return (body[sp + 8..sp + 8 + se].trim().to_string(), lead + 7 + e + 8);
                }
            }
            return (String::new(), lead + 7 + e + 8);
        }
    }
    (String::new(), lead)
}

fn scan_dir(dir: &std::path::Path, domain: &str, out: &mut Vec<ServiceInfo>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("plist") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let mut s = parse_plist(&text);
        if s.label.is_empty() {
            s.label = p.file_stem().and_then(|x| x.to_str()).unwrap_or_default().to_string();
        }
        out.push(ServiceInfo {
            label: s.label,
            pid: 0,
            last_exit_status: 0,
            program: s.program,
            run_at_load: s.run_at_load,
            keep_alive: s.keep_alive,
            disabled: s.disabled,
            domain: domain.to_string(),
            plist_path: p.display().to_string(),
        });
    }
}

pub fn services() -> Vec<ServiceInfo> {
    let live = launchctl_list();
    let disabled = disabled_labels();
    let mut out: Vec<ServiceInfo> = Vec::new();

    let home = std::env::var("HOME").unwrap_or_default();
    let user_agents = PathBuf::from(&home).join("Library/LaunchAgents");
    for d in LAUNCH_DIRS_SYSTEM {
        scan_dir(std::path::Path::new(d), "system", &mut out);
    }
    for d in LAUNCH_DIRS_AGENT {
        scan_dir(std::path::Path::new(d), "user", &mut out);
    }
    scan_dir(&user_agents, &format!("gui/{}", unsafe { super::ffi::geteuid() }), &mut out);

    // merge runtime state; include loaded jobs that have no visible plist
    for svc in &mut out {
        if let Some(&(pid, status)) = live.get(&svc.label) {
            svc.pid = pid;
            svc.last_exit_status = status;
        }
        if let Some(&d) = disabled.get(&svc.label) {
            svc.disabled = svc.disabled || d;
        }
    }
    for (label, &(pid, status)) in &live {
        if !out.iter().any(|s| &s.label == label) {
            out.push(ServiceInfo {
                label: label.clone(),
                pid,
                last_exit_status: status,
                domain: "loaded".into(),
                ..Default::default()
            });
        }
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

/// Control a launchd job (SI: start/stop/pause/continue/restart + enable/disable).
pub fn service_action(label: &str, plist_path: &str, domain: &str, action: ServiceAction) -> Result<String, String> {
    let uid = unsafe { super::ffi::geteuid() };
    let dom = if domain == "system" { "system".to_string() } else { format!("gui/{uid}") };
    let target = format!("{dom}/{label}");
    let args: Vec<String> = match action {
        ServiceAction::Start => {
            if !plist_path.is_empty() && std::path::Path::new(plist_path).exists() {
                vec!["bootstrap".into(), dom.clone(), plist_path.into()]
            } else {
                vec!["kickstart".into(), target.clone()]
            }
        }
        ServiceAction::Stop => vec!["kill".into(), "TERM".into(), target.clone()],
        ServiceAction::Kickstart => vec!["kickstart".into(), "-k".into(), target.clone()],
        ServiceAction::Enable => vec!["enable".into(), target.clone()],
        ServiceAction::Disable => vec!["disable".into(), target.clone()],
    };
    let out = std::process::Command::new("/bin/launchctl")
        .args(&args)
        .output()
        .map_err(|e| format!("launchctl: {e}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(format!("launchctl {} → OK", args.join(" ")))
    } else {
        Err(format!("launchctl {} failed: {stderr}", args.join(" ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_parse_basic() {
        let xml = r#"<?xml version="1.0"?>
        <plist version="1.0"><dict>
            <key>Label</key><string>com.example.agent</string>
            <key>ProgramArguments</key>
            <array><string>/usr/local/bin/agent</string><string>--flag</string></array>
            <key>RunAtLoad</key><true/>
            <key>KeepAlive</key><false/>
        </dict></plist>"#;
        let s = parse_plist(xml);
        assert_eq!(s.label, "com.example.agent");
        assert_eq!(s.program, "/usr/local/bin/agent");
        assert!(s.run_at_load);
        assert!(!s.keep_alive);
    }

    #[test]
    fn plist_parse_program_key() {
        let xml = "<dict><key>Label</key><string>com.x</string><key>Program</key><string>/bin/echo</string><key>Disabled</key><true/></dict>";
        let s = parse_plist(xml);
        assert_eq!(s.program, "/bin/echo");
        assert!(s.disabled);
    }

    #[test]
    fn launchctl_list_parse_shape() {
        // pure parser sanity via a synthetic table
        let text = "PID\tStatus\tLabel\n123\t0\tcom.apple.test\n-\t-78\tcom.example.stopped\n";
        let mut map = HashMap::new();
        for line in text.lines().skip(1) {
            let mut f = line.split('\t');
            let (Some(p), Some(s), Some(l)) = (f.next(), f.next(), f.next()) else { continue };
            map.insert(l.to_string(), (p.parse().unwrap_or(0), s.parse().unwrap_or(0)));
        }
        assert_eq!(map["com.apple.test"], (123, 0));
        assert_eq!(map["com.example.stopped"], (0, -78));
    }
}
