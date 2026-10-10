//! Minimal Objective-C runtime FFI — just enough of libobjc + AppKit to run
//! an NSStatusItem app without linking any crate. Same technique the
//! dependency-free macOS ecosystem uses: `objc_msgSend` is declared once and
//! transmuted to correctly-typed call sites (all selectors used here have
//! simple signatures; no stret/variadic cases).

#![cfg(target_os = "macos")]
#![allow(non_snake_case, dead_code)]

use core::ffi::c_char;

pub type Id = *mut core::ffi::c_void;
pub type Sel = *mut core::ffi::c_void;
pub type Class = *mut core::ffi::c_void;
pub type Imp = *const core::ffi::c_void;

pub const NIL: Id = core::ptr::null_mut();

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NSPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NSSize {
    pub w: f64,
    pub h: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NSRect {
    pub origin: NSPoint,
    pub size: NSSize,
}

impl NSRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        NSRect { origin: NSPoint { x, y }, size: NSSize { w, h } }
    }
}

unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Class;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend(); // address transmuted to typed call sites
    fn objc_allocateClassPair(superclass: Class, name: *const c_char, extra_bytes: usize) -> Class;
    fn class_addMethod(cls: Class, name: Sel, imp: Imp, types: *const c_char) -> u8;
    fn objc_registerClassPair(cls: Class);
}

pub fn class(name: &str) -> Class {
    let c = std::ffi::CString::new(name).unwrap();
    unsafe { objc_getClass(c.as_ptr()) }
}

pub fn sel(name: &str) -> Sel {
    let c = std::ffi::CString::new(name).unwrap();
    unsafe { sel_registerName(c.as_ptr()) }
}

// ---- typed message sends -------------------------------------------------

pub unsafe fn send0(target: Id, s: Sel) -> Id {
    let f: unsafe extern "C" fn(Id, Sel) -> Id = core::mem::transmute(objc_msgSend as *const ());
    f(target, s)
}

pub unsafe fn send0_void(target: Id, s: Sel) {
    let f: unsafe extern "C" fn(Id, Sel) = core::mem::transmute(objc_msgSend as *const ());
    f(target, s)
}

pub unsafe fn send0_i64(target: Id, s: Sel) -> i64 {
    let f: unsafe extern "C" fn(Id, Sel) -> i64 = core::mem::transmute(objc_msgSend as *const ());
    f(target, s)
}

pub unsafe fn send1_id(target: Id, s: Sel, a: Id) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, Id) -> Id = core::mem::transmute(objc_msgSend as *const ());
    f(target, s, a)
}

pub unsafe fn send1_id_void(target: Id, s: Sel, a: Id) {
    let f: unsafe extern "C" fn(Id, Sel, Id) = core::mem::transmute(objc_msgSend as *const ());
    f(target, s, a)
}

pub unsafe fn send1_sel(target: Id, s: Sel, a: Sel) {
    let f: unsafe extern "C" fn(Id, Sel, Sel) = core::mem::transmute(objc_msgSend as *const ());
    f(target, s, a)
}

pub unsafe fn send1_i64(target: Id, s: Sel, a: i64) {
    let f: unsafe extern "C" fn(Id, Sel, i64) = core::mem::transmute(objc_msgSend as *const ());
    f(target, s, a)
}

pub unsafe fn send1_f64(target: Id, s: Sel, a: f64) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, f64) -> Id = core::mem::transmute(objc_msgSend as *const ());
    f(target, s, a)
}

pub unsafe fn send1_size(target: Id, s: Sel, a: NSSize) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, NSSize) -> Id = core::mem::transmute(objc_msgSend as *const ());
    f(target, s, a)
}

pub unsafe fn send1_bool(target: Id, s: Sel, a: bool) {
    let f: unsafe extern "C" fn(Id, Sel, bool) = core::mem::transmute(objc_msgSend as *const ());
    f(target, s, a)
}

pub unsafe fn send_rect(cls: Id, s: Sel, r: NSRect) {
    let f: unsafe extern "C" fn(Id, Sel, NSRect) = core::mem::transmute(objc_msgSend as *const ());
    f(cls, s, r)
}

pub unsafe fn send_color(cls: Id, s: Sel, r: f64, g: f64, b: f64, a: f64) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, f64, f64, f64, f64) -> Id = core::mem::transmute(objc_msgSend as *const ());
    f(cls, s, r, g, b, a)
}

pub unsafe fn send_timer(cls: Id, s: Sel, interval: f64, target: Id, timer_sel: Sel, user_info: Id, repeats: bool) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, f64, Id, Sel, Id, bool) -> Id = core::mem::transmute(objc_msgSend as *const ());
    f(cls, s, interval, target, timer_sel, user_info, repeats)
}

pub unsafe fn send_menu_item(cls: Id, s: Sel, title: Id, action: Sel, key_equiv: Id) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, Id, Sel, Id) -> Id = core::mem::transmute(objc_msgSend as *const ());
    f(cls, s, title, action, key_equiv)
}

// ---- helpers -------------------------------------------------------------

/// Register a delegate class at runtime with Rust functions as methods.
pub fn make_class(name: &str, superclass: &str, methods: &[(&str, Imp, &str)]) -> Class {
    let sup = class(superclass);
    let cname = std::ffi::CString::new(name).unwrap();
    let cls = unsafe { objc_allocateClassPair(sup, cname.as_ptr(), 0) };
    assert!(!cls.is_null(), "failed to allocate ObjC class {name}");
    for (mname, imp, types) in methods {
        let t = std::ffi::CString::new(*types).unwrap();
        let ok = unsafe { class_addMethod(cls, sel(mname), *imp, t.as_ptr()) };
        assert!(ok != 0, "failed to add method {mname} to {name}");
    }
    unsafe { objc_registerClassPair(cls) };
    cls
}

/// `[NSString stringWithUTF8String:]` — autoreleased, valid within the pool.
pub fn nsstr(s: &str) -> Id {
    let c = std::ffi::CString::new(s).unwrap_or_else(|_| std::ffi::CString::new("?").unwrap());
    unsafe { send1_id(class("NSString"), sel("stringWithUTF8String:"), c.as_ptr() as Id) }
}

pub fn alloc(cls: &str) -> Id {
    unsafe { send0(class(cls), sel("alloc")) }
}

/// RAII autorelease pool so per-tick Cocoa objects don't accumulate.
pub struct Pool(Id);

impl Pool {
    pub fn new() -> Pool {
        let p = alloc("NSAutoreleasePool");
        Pool(unsafe { send0(p, sel("init")) })
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        unsafe { send0_void(self.0, sel("drain")) };
    }
}
