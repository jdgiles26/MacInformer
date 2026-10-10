//! Double-buffered cell grid with diff-based ANSI rendering.

/// 256-color palette index, or -1 for terminal default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub fg: i16,
    pub bg: i16,
    pub bold: bool,
    pub dim: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style { fg: -1, bg: -1, bold: false, dim: false }
    }
}

impl Style {
    pub const fn new(fg: i16, bg: i16) -> Style {
        Style { fg, bg, bold: false, dim: false }
    }
    pub const fn bold(mut self) -> Style {
        self.bold = true;
        self
    }
    pub const fn dim(mut self) -> Style {
        self.dim = true;
        self
    }
    fn ansi(&self) -> String {
        let mut s = String::from("\x1b[0");
        if self.bold {
            s.push_str(";1");
        }
        if self.dim {
            s.push_str(";2");
        }
        if self.fg >= 0 {
            s.push_str(&format!(";38;5;{}", self.fg));
        }
        if self.bg >= 0 {
            s.push_str(&format!(";48;5;{}", self.bg));
        }
        s.push('m');
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', style: Style::default() }
    }
}

#[derive(Clone)]
pub struct Buffer {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<Cell>,
}

impl Buffer {
    pub fn new(w: usize, h: usize) -> Buffer {
        Buffer { w, h, cells: vec![Cell::default(); w * h] }
    }

    pub fn clear(&mut self) {
        for c in &mut self.cells {
            *c = Cell::default();
        }
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, ch: char, style: Style) {
        if x < self.w && y < self.h {
            self.cells[y * self.w + x] = Cell { ch, style };
        }
    }

    pub fn set_str(&mut self, x: usize, y: usize, s: &str, style: Style) {
        for (i, ch) in s.chars().enumerate() {
            self.set(x + i, y, ch, style);
        }
    }

    /// Right-aligned string ending at column `x_end` (exclusive).
    pub fn set_str_right(&mut self, x_end: usize, y: usize, s: &str, style: Style) {
        let n = s.chars().count();
        if n <= x_end {
            self.set_str(x_end - n, y, s, style);
        }
    }

    /// Truncating cell writer for table columns: writes `s` into [x, x+w).
    pub fn set_cell(&mut self, x: usize, y: usize, w: usize, s: &str, style: Style) {
        let total = s.chars().count();
        for (n, ch) in s.chars().enumerate() {
            if n >= w.saturating_sub(1) && total > w {
                self.set(x + n, y, '…', style);
                return;
            }
            if n >= w {
                return;
            }
            self.set(x + n, y, ch, style);
        }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, style: Style) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                let c = &mut self.cells[yy * self.w + xx];
                c.style = style;
            }
        }
    }

    pub fn hline(&mut self, x: usize, y: usize, w: usize, style: Style) {
        for xx in x..(x + w).min(self.w) {
            self.set(xx, y, '─', style);
        }
    }

    pub fn vline(&mut self, x: usize, y: usize, h: usize, style: Style) {
        for yy in y..(y + h).min(self.h) {
            self.set(x, yy, '│', style);
        }
    }

    pub fn draw_box(&mut self, x: usize, y: usize, w: usize, h: usize, style: Style) {
        self.set(x, y, '┌', style);
        self.set(x + w - 1, y, '┐', style);
        self.set(x, y + h - 1, '└', style);
        self.set(x + w - 1, y + h - 1, '┘', style);
        self.hline(x + 1, y, w - 2, style);
        self.hline(x + 1, y + h - 1, w - 2, style);
        self.vline(x, y + 1, h - 2, style);
        self.vline(x + w - 1, y + 1, h - 2, style);
    }

    /// Diff-render against `prev`, emitting minimal ANSI. After the call,
    /// `prev` holds a copy of `self`.
    pub fn flush_diff(&self, prev: &mut Buffer, out: &mut String) {
        out.push_str("\x1b[H");
        let mut cur = Style { fg: -2, bg: -2, bold: false, dim: false }; // force first emit
        for y in 0..self.h.min(prev.h) {
            let mut x = 0;
            while x < self.w.min(prev.w) {
                let c = self.cells[y * self.w + x];
                let p = prev.cells[y * prev.w + x];
                if c == p {
                    x += 1;
                    continue;
                }
                out.push_str(&format!("\x1b[{};{}H", y + 1, x + 1));
                if c.style != cur {
                    out.push_str(&c.style.ansi());
                    cur = c.style;
                }
                // run of same-style changed cells
                let mut end = x;
                while end < self.w.min(prev.w) {
                    let c2 = self.cells[y * self.w + end];
                    if c2 == prev.cells[y * prev.w + end] || c2.style != c.style {
                        break;
                    }
                    out.push(if c2.ch == '\0' { ' ' } else { c2.ch });
                    end += 1;
                }
                x = end;
            }
        }
        out.push_str("\x1b[0m");
        *prev = self.clone();
    }

    /// Plain-text dump (for --dump mode and tests).
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for y in 0..self.h {
            let row: String = self.cells[y * self.w..(y + 1) * self.w].iter().map(|c| if c.ch == '\0' { ' ' } else { c.ch }).collect();
            s.push_str(row.trim_end());
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_dump() {
        let mut b = Buffer::new(20, 3);
        b.set_str(1, 1, "hello", Style::default());
        let t = b.to_text();
        assert!(t.lines().nth(1).unwrap().starts_with(" hello"));
    }

    #[test]
    fn right_align() {
        let mut b = Buffer::new(10, 1);
        b.set_str_right(10, 0, "42%", Style::default());
        assert!(b.to_text().trim_end().ends_with("42%"));
    }

    #[test]
    fn diff_only_changes() {
        let mut a = Buffer::new(10, 2);
        let mut prev = Buffer::new(10, 2);
        a.set_str(0, 0, "abc", Style::default());
        let mut out = String::new();
        a.flush_diff(&mut prev, &mut out);
        assert!(out.contains('a'));
        let mut out2 = String::new();
        a.flush_diff(&mut prev, &mut out2);
        assert!(!out2.contains('a'), "no changes → no cell output");
    }
}
