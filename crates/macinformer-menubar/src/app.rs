//! The menu-bar application: an NSStatusItem with a live CPU graph icon and
//! a dropdown of always-fresh numbers (memory, GPU, sensors, battery,
//! network, top processes, alerts). Wraps `macinformer-core` — same data the
//! TUI shows, zero additional dependencies, everything drawn with AppKit.
//!
//! Layout: the status item icon is a 26×18 px bar graph of recent CPU usage
//! (template image, so it adapts to light/dark menu bar); the menu is a set
//! of NSMenuItems whose titles are rewritten every second by an NSTimer that
//! fires on the main run loop. All ObjC objects live on the main thread;
//! Rust state sits in a thread_local, so no locking exists anywhere.

#![cfg(target_os = "macos")]

use crate::graph;
use crate::objc::*;
use macinformer_core::*;
use std::cell::RefCell;

const GRAPH_W: usize = 26;
const GRAPH_H: usize = 18;
const HIST: usize = GRAPH_W;

struct Items {
    cpu: Id,
    memory: Id,
    gpu: Id,
    sensors: Id,
    battery: Id,
    network: Id,
    alerts: Id,
    top: Vec<Id>,
}

struct MenuApp {
    col: Box<dyn Collector>,
    status_item: Id,
    items: Items,
    cpu_hist: Vec<f32>,
    alert_cfg: AlertConfig,
    alert_tracker: AlertTracker,
    tui_path: Option<String>,
}

thread_local! {
    static APP: RefCell<Option<MenuApp>> = const { RefCell::new(None) };
}

pub fn run() -> ! {
    unsafe {
        let _pool = Pool::new();
        let app = send0(class("NSApplication"), sel("sharedApplication"));
        // NSApplicationActivationPolicyAccessory = 1 (menu-bar-only, no Dock icon)
        send1_i64(app, sel("setActivationPolicy:"), 1);

        let delegate_cls = make_class(
            "MacInformerDelegate",
            "NSObject",
            &[
                ("applicationDidFinishLaunching:", did_finish_launching as Imp, "v@:@"),
                ("miTick:", on_tick as Imp, "v@:@"),
                ("miMenuAction:", on_menu_action as Imp, "v@:@"),
            ],
        );
        let delegate = send0(delegate_cls as Id, sel("alloc"));
        let delegate = send0(delegate, sel("init"));
        send1_id_void(app, sel("setDelegate:"), delegate);
        send0_void(app, sel("run"));
    }
    std::process::exit(0);
}

extern "C" fn did_finish_launching(_this: Id, _cmd: Sel, _note: Id) {
    unsafe {
        // status item
        let bar = send0(class("NSStatusBar"), sel("systemStatusBar"));
        let item = send1_f64(bar, sel("statusItemWithLength:"), -1.0); // variable length
        send0(item, sel("retain"));

        let menu = send0(alloc("NSMenu"), sel("init"));
        send1_bool(menu, sel("setAutoenablesItems:"), false);

        // live stat rows (no action)
        let none: Sel = core::ptr::null_mut();
        let mi = |menu: Id, title: &str| -> Id {
            let item = send_menu_item(alloc("NSMenuItem"), sel("initWithTitle:action:keyEquivalent:"), nsstr(title), none, nsstr(""));
            send1_bool(item, sel("setEnabled:"), false);
            send1_id_void(menu, sel("addItem:"), item);
            item
        };
        let header = mi(menu, "MacInformer — warming up…");
        send1_bool(header, sel("setEnabled:"), false);
        let _ = header;

        let cpu = mi(menu, "CPU: —");
        let memory = mi(menu, "Memory: —");
        let gpu = mi(menu, "GPU: —");
        let sensors = mi(menu, "Temp: —");
        let battery = mi(menu, "Battery: —");
        let network = mi(menu, "Network: —");
        send1_id_void(menu, sel("addItem:"), send0(class("NSMenuItem"), sel("separatorItem")));
        let mut top = Vec::new();
        for i in 0..5 {
            top.push(mi(menu, &format!("{}. —", i + 1)));
        }
        send1_id_void(menu, sel("addItem:"), send0(class("NSMenuItem"), sel("separatorItem")));
        let alerts = mi(menu, "Alerts: none");

        // action rows
        let delegate = send0(send0(class("NSApplication"), sel("sharedApplication")), sel("delegate"));
        send1_id_void(menu, sel("addItem:"), send0(class("NSMenuItem"), sel("separatorItem")));
        let open = send_menu_item(alloc("NSMenuItem"), sel("initWithTitle:action:keyEquivalent:"), nsstr("Open MacInformer TUI"), sel("miMenuAction:"), nsstr(""));
        send1_id_void(open, sel("setTarget:"), delegate);
        send1_i64(open, sel("setTag:"), 1);
        send1_id_void(menu, sel("addItem:"), open);
        let quit = send_menu_item(alloc("NSMenuItem"), sel("initWithTitle:action:keyEquivalent:"), nsstr("Quit MacInformer"), sel("miMenuAction:"), nsstr("q"));
        send1_id_void(quit, sel("setTarget:"), delegate);
        send1_i64(quit, sel("setTag:"), 2);
        send1_id_void(menu, sel("addItem:"), quit);

        send1_id_void(item, sel("setMenu:"), menu);

        APP.with(|a| {
            *a.borrow_mut() = Some(MenuApp {
                col: system_collector(),
                status_item: item,
                items: Items { cpu, memory, gpu, sensors, battery, network, alerts, top },
                cpu_hist: Vec::with_capacity(HIST),
                alert_cfg: AlertConfig::default(),
                alert_tracker: AlertTracker::default(),
                tui_path: find_tui_binary(),
            });
        });

        // 1 Hz refresh timer on the main run loop
        let delegate2 = send0(send0(class("NSApplication"), sel("sharedApplication")), sel("delegate"));
        send_timer(
            class("NSTimer"),
            sel("scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
            1.0,
            delegate2,
            sel("miTick:"),
            NIL,
            true,
        );
        // paint immediately rather than after the first second
        on_tick(delegate2, sel("miTick:"), NIL);
    }
}

extern "C" fn on_tick(_this: Id, _cmd: Sel, _timer: Id) {
    let _pool = Pool::new();
    APP.with(|a| {
        let mut borrow = a.borrow_mut();
        let Some(app) = borrow.as_mut() else { return };
        let snap = app.col.snapshot();
        let sensors = read_sensors();
        let battery = read_battery();

        // history for the icon graph
        app.cpu_hist.push(snap.cpu.total);
        if app.cpu_hist.len() > HIST {
            app.cpu_hist.remove(0);
        }

        // icon: CPU history bars + compact percent title
        let bars = graph::bars_from(&app.cpu_hist, 100.0, GRAPH_W, GRAPH_H);
        let img = draw_graph_image(&bars);
        unsafe {
            let button = send0(app.status_item, sel("button"));
            send1_id_void(button, sel("setImage:"), img);
            send1_id_void(button, sel("setTitle:"), nsstr(&graph::title_for(snap.cpu.total)));
            // image left of the title: NSImageLeft = 2
            send1_i64(button, sel("setImagePosition:"), 2);
            send1_id_void(button, sel("setToolTip:"), nsstr("MacInformer — CPU usage history"));
        }

        // menu rows
        unsafe {
            send1_id_void(app.items.cpu, sel("setTitle:"), nsstr(&graph::line_cpu(&snap)));
            send1_id_void(app.items.memory, sel("setTitle:"), nsstr(&graph::line_memory(&snap)));
            send1_id_void(app.items.gpu, sel("setTitle:"), nsstr(&graph::line_gpu(&snap)));
            send1_id_void(app.items.sensors, sel("setTitle:"), nsstr(&graph::line_sensors(&sensors)));
            send1_id_void(app.items.battery, sel("setTitle:"), nsstr(&graph::line_battery(&battery)));
            send1_id_void(app.items.network, sel("setTitle:"), nsstr(&graph::line_network(&snap)));
        }
        let tops = graph::lines_top(&snap.processes, app.items.top.len());
        for (i, id) in app.items.top.iter().enumerate() {
            let line = tops.get(i).cloned().unwrap_or_else(|| format!("{}. —", i + 1));
            unsafe { send1_id_void(*id, sel("setTitle:"), nsstr(&line)) };
        }

        // alerts: same engine as the TUI + notification delivery
        let now = evaluate_alerts(&app.alert_cfg, &snap, &sensors, &battery);
        let label = if now.is_empty() { "Alerts: none".to_string() } else { format!("Alerts: {} raised — {}", now.len(), now[0].message) };
        unsafe { send1_id_void(app.items.alerts, sel("setTitle:"), nsstr(&label)) };
        for msg in app.alert_tracker.update(&now) {
            notify_macos("MacInformer alert", &msg);
        }
    });
}

extern "C" fn on_menu_action(_this: Id, _cmd: Sel, sender: Id) {
    match unsafe { send0_i64(sender, sel("tag")) } {
        1 => {
            // Open MacInformer TUI in a fresh Terminal window
            if let Some(path) = APP.with(|a| a.borrow().as_ref().and_then(|m| m.tui_path.clone())) {
                let _ = std::process::Command::new("/usr/bin/open").args(["-a", "Terminal", &path]).spawn();
            }
        }
        2 => unsafe {
            let app = send0(class("NSApplication"), sel("sharedApplication"));
            send1_id_void(app, sel("terminate:"), NIL);
        },
        _ => {}
    }
}

/// Draw the status-item graph: a 26×18 template image of CPU history bars.
fn draw_graph_image(bars: &[usize]) -> Id {
    unsafe {
        let img = send1_size(alloc("NSImage"), sel("initWithSize:"), NSSize { w: GRAPH_W as f64, h: GRAPH_H as f64 });
        send0_void(img, sel("lockFocus"));
        // template images render only the alpha channel in the menu bar
        let black = send0(class("NSColor"), sel("blackColor"));
        send0_void(black, sel("set"));
        for (x, &h) in bars.iter().enumerate() {
            if h == 0 {
                continue;
            }
            send_rect(class("NSBezierPath"), sel("fillRect:"), NSRect::new(x as f64, 0.0, 1.0, h as f64));
        }
        send0_void(img, sel("unlockFocus"));
        send1_bool(img, sel("setTemplate:"), true);
        img
    }
}

/// Locate the sibling `macinformer` TUI binary (same dir as this exe, else PATH).
fn find_tui_binary() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("macinformer");
            if cand.exists() {
                return Some(cand.to_string_lossy().into_owned());
            }
        }
    }
    let out = std::process::Command::new("/usr/bin/which").arg("macinformer").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && !p.is_empty() { Some(p) } else { None }
}

/// Best-effort macOS notification via osascript (no app bundle required).
fn notify_macos(title: &str, msg: &str) {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\"", esc(msg), esc(title));
    let _ = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).spawn();
}
