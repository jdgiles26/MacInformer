//! Raw terminal handling via termios ioctl — no ncurses, no dependencies.

use std::io::Write;

#[cfg(target_os = "linux")]
mod os {
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: u32,
        pub c_oflag: u32,
        pub c_cflag: u32,
        pub c_lflag: u32,
        pub c_line: u8,
        pub c_cc: [u8; 32],
        pub c_ispeed: u32,
        pub c_ospeed: u32,
    }
    pub const ICANON: u32 = 0x2;
    pub const ECHO: u32 = 0x8;
    pub const ISIG: u32 = 0x1;
    pub const IEXTEN: u32 = 0x8000;
    pub const IXON: u32 = 0x400;
    pub const ICRNL: u32 = 0x100;
    pub const OPOST: u32 = 0x1;
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;
    pub const TCSANOW: i32 = 0;
    pub const TIOCGWINSZ: usize = 0x5413;
}

#[cfg(target_os = "macos")]
mod os {
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: u64,
        pub c_oflag: u64,
        pub c_cflag: u64,
        pub c_lflag: u64,
        pub c_cc: [u8; 20],
        pub c_ispeed: u64,
        pub c_ospeed: u64,
    }
    pub const ICANON: u64 = 0x100;
    pub const ECHO: u64 = 0x8;
    pub const ISIG: u64 = 0x80;
    pub const IEXTEN: u64 = 0x400;
    pub const IXON: u64 = 0x200;
    pub const ICRNL: u64 = 0x100;
    pub const OPOST: u64 = 0x1;
    pub const VMIN: usize = 16;
    pub const VTIME: usize = 17;
    pub const TCSANOW: i32 = 0;
    pub const TIOCGWINSZ: usize = 0x4008_7468;
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod os {
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: u64,
        pub c_oflag: u64,
        pub c_cflag: u64,
        pub c_lflag: u64,
        pub c_cc: [u8; 20],
        pub c_ispeed: u64,
        pub c_ospeed: u64,
    }
    pub const ICANON: u64 = 0x100;
    pub const ECHO: u64 = 0x8;
    pub const ISIG: u64 = 0x80;
    pub const IEXTEN: u64 = 0x400;
    pub const IXON: u64 = 0x200;
    pub const ICRNL: u64 = 0x100;
    pub const OPOST: u64 = 0x1;
    pub const VMIN: usize = 16;
    pub const VTIME: usize = 17;
    pub const TCSANOW: i32 = 0;
    pub const TIOCGWINSZ: usize = 0x4008_7468;
}

use os::*;

#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

unsafe extern "C" {
    fn tcgetattr(fd: i32, t: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, act: i32, t: *const Termios) -> i32;
    fn ioctl(fd: i32, req: usize, ...) -> i32;
    #[link_name = "read"]
    fn posix_read(fd: i32, buf: *mut core::ffi::c_void, n: usize) -> isize;
    fn isatty(fd: i32) -> i32;
}

pub struct Term {
    orig: Option<Termios>,
    pub w: usize,
    pub h: usize,
    out: std::io::Stdout,
}

impl Term {
    /// Enter raw mode + alternate screen. Returns Err when stdin is not a TTY
    /// (in which case the app can fall back to --dump mode).
    pub fn new() -> std::io::Result<Term> {
        if unsafe { isatty(0) } != 1 {
            return Err(std::io::Error::other("stdin is not a TTY"));
        }
        let mut orig: Termios = unsafe { core::mem::zeroed() };
        if unsafe { tcgetattr(0, &mut orig) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut raw = orig;
        raw.c_lflag &= !(ICANON | ECHO | ISIG | IEXTEN);
        raw.c_iflag &= !(IXON | ICRNL);
        raw.c_oflag &= !OPOST;
        raw.c_cc[VMIN] = 0;
        raw.c_cc[VTIME] = 1; // 0.1 s read timeout → input polling cadence
        if unsafe { tcsetattr(0, TCSANOW, &raw) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut t = Term { orig: Some(orig), w: 0, h: 0, out: std::io::stdout() };
        t.update_size();
        t.write("\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H");
        Ok(t)
    }

    pub fn update_size(&mut self) -> bool {
        let mut ws = Winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
        unsafe { ioctl(0, TIOCGWINSZ, &mut ws) };
        let (w, h) = (ws.ws_col as usize, ws.ws_row as usize);
        let changed = w != self.w || h != self.h;
        self.w = w.max(40);
        self.h = h.max(10);
        changed
    }

    /// Non-blocking read (VTIME=1 → at most ~0.1 s wait).
    pub fn read_keys(&mut self) -> Vec<u8> {
        let mut buf = [0u8; 256];
        let n = unsafe { posix_read(0, buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 { buf[..n as usize].to_vec() } else { Vec::new() }
    }

    pub fn write(&mut self, s: &str) {
        let _ = self.out.write_all(s.as_bytes());
        let _ = self.out.flush();
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        if let Some(orig) = self.orig.take() {
            unsafe { tcsetattr(0, TCSANOW, &orig) };
        }
        self.write("\x1b[?25h\x1b[?1049l\x1b[0m");
    }
}
