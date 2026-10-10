//! Reusable widgets: sortable/tree table, area graphs, tabs, dialogs.
//! Visual theme mirrors System Informer's dark mode and highlight colors.

use crate::buffer::{Buffer, Style};

// ------------------------------------------------------------- theme (SI)

pub const C_HEADER_BG: i16 = 236;
pub const C_HEADER_FG: i16 = 250;
pub const C_TAB_ACTIVE_BG: i16 = 31;
pub const C_SELECTED_BG: i16 = 24;
pub const C_NEW_BG: i16 = 22; // SI green flash for new processes
pub const C_DEAD_BG: i16 = 52; // SI red flash for dead processes
pub const C_SYSTEM_FG: i16 = 39; // SI blue tint for system processes
pub const C_OWN_FG: i16 = 229; // SI yellow tint for own processes
pub const C_DIM_FG: i16 = 244;
pub const C_ACCENT: i16 = 75;

pub const C_CPU: i16 = 34;
pub const C_MEM: i16 = 178;
pub const C_IO_TOTAL: i16 = 220;
pub const C_IO_READ: i16 = 127;
pub const C_GPU: i16 = 40;
pub const C_DISK: i16 = 133;
pub const C_NET: i16 = 220;

pub const S_HEADER: Style = Style::new(C_HEADER_FG, C_HEADER_BG);
pub const S_SELECTED: Style = Style::new(15, C_SELECTED_BG);
pub const S_SYSTEM: Style = Style::new(C_SYSTEM_FG, -1);
pub const S_OWN: Style = Style::new(C_OWN_FG, -1);
pub const S_NEW: Style = Style::new(15, C_NEW_BG);
pub const S_DEAD: Style = Style::new(15, C_DEAD_BG);
pub const S_DIM: Style = Style::new(C_DIM_FG, -1);
pub const S_NORMAL: Style = Style { fg: -1, bg: -1, bold: false, dim: false };
pub const S_BOLD: Style = Style { fg: -1, bg: -1, bold: true, dim: false };

// ------------------------------------------------------------------ table

#[derive(Clone)]
pub struct Column {
    pub title: String,
    pub width: usize,
    pub flex: bool,
    pub right: bool,
}

impl Column {
    pub fn new(t: &str, w: usize) -> Column {
        Column { title: t.into(), width: w, flex: false, right: false }
    }
    pub fn right(t: &str, w: usize) -> Column {
        Column { title: t.into(), width: w, flex: false, right: true }
    }
    pub fn flex(t: &str) -> Column {
        Column { title: t.into(), width: 10, flex: true, right: false }
    }
}

#[derive(Clone)]
pub struct Row {
    pub id: i64,
    pub cells: Vec<String>,
    /// Numeric sort keys parallel to `cells` (f64::MIN for missing).
    pub numeric: Vec<f64>,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub style: Style,
}

#[derive(Clone)]
pub struct Table {
    pub columns: Vec<Column>,
    pub sort_col: usize,
    pub sort_asc: bool,
    pub selected: usize,
    pub scroll: usize,
}

impl Table {
    pub fn new(columns: Vec<Column>, sort_col: usize) -> Table {
        Table { columns, sort_col, sort_asc: true, selected: 0, scroll: 0 }
    }

    pub fn move_sel(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.selected = 0;
            return;
        }
        let n = self.selected as isize + delta;
        self.selected = n.clamp(0, len as isize - 1) as usize;
    }

    pub fn page(&mut self, delta: isize, len: usize, page: usize) {
        self.move_sel(delta * page.max(1) as isize, len);
    }

    pub fn sort_by(&mut self, col: usize) {
        if col >= self.columns.len() {
            return;
        }
        if self.sort_col == col {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_col = col;
            self.sort_asc = true;
        }
    }

    pub fn ensure_visible(&mut self, view_h: usize, len: usize) {
        if self.selected >= len && len > 0 {
            self.selected = len - 1;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if view_h > 0 && self.selected >= self.scroll + view_h {
            self.scroll = self.selected + 1 - view_h;
        }
    }

    fn column_x(&self, total_w: usize) -> Vec<(usize, usize)> {
        let fixed: usize = self.columns.iter().filter(|c| !c.flex).map(|c| c.width + 1).sum();
        let flex_n = self.columns.iter().filter(|c| c.flex).count();
        let flex_w = total_w.saturating_sub(fixed).checked_div(flex_n).unwrap_or(0);
        let mut x = 0;
        self.columns
            .iter()
            .map(|c| {
                let w = if c.flex { flex_w.max(6) } else { c.width };
                let r = (x, w);
                x += w + 1;
                r
            })
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_table(buf: &mut Buffer, x: usize, y: usize, w: usize, h: usize, table: &Table, rows: &[Row], tree: bool) {
    if h < 2 || w < 8 {
        return;
    }
    let xs = table.column_x(w);
    // header
    buf.fill_rect(x, y, w, 1, S_HEADER);
    for (i, col) in table.columns.iter().enumerate() {
        let (cx, cw) = xs[i];
        if cx >= w {
            break;
        }
        let arrow = if i == table.sort_col {
            if table.sort_asc { " ▲" } else { " ▼" }
        } else {
            ""
        };
        let title = format!("{}{}", col.title, arrow);
        if col.right {
            buf.set_str_right(x + cx + cw, y, &title, S_HEADER.bold());
        } else {
            buf.set_cell(x + cx, y, cw, &title, S_HEADER.bold());
        }
    }
    // rows
    let view_h = h - 1;
    for (vi, row) in rows.iter().skip(table.scroll).take(view_h).enumerate() {
        let ry = y + 1 + vi;
        let sel = table.scroll + vi == table.selected;
        let base = if sel {
            Style { bg: C_SELECTED_BG, fg: if row.style.fg >= 0 { row.style.fg } else { 15 }, bold: false, dim: false }
        } else {
            row.style
        };
        if sel || base.bg >= 0 {
            buf.fill_rect(x, ry, w, 1, base);
        }
        for (i, cell) in row.cells.iter().enumerate() {
            if i >= xs.len() {
                break;
            }
            let (cx, cw) = xs[i];
            if cx >= w {
                break;
            }
            if i == 0 && tree {
                let indent = row.depth * 2;
                let glyph = if row.has_children {
                    if row.expanded { "▾ " } else { "▸ " }
                } else {
                    "  "
                };
                buf.set_str(x + cx + indent, ry, glyph, base);
                if col_fits(indent + 2, cw) {
                    buf.set_cell(x + cx + indent + 2, ry, cw - indent - 2, cell, base);
                }
            } else if table.columns[i].right {
                let s = truncate(cell, cw);
                buf.set_str_right(x + cx + cw.min(w - cx), ry, &s, base);
            } else {
                buf.set_cell(x + cx, ry, cw.min(w - cx), cell, base);
            }
        }
    }
    // scrollbar hint
    if rows.len() > view_h {
        let frac = table.scroll as f32 / (rows.len() - view_h).max(1) as f32;
        let sy = y + 1 + (frac * (view_h.saturating_sub(1)) as f32) as usize;
        buf.set(x + w - 1, sy, '█', S_DIM);
    }
}

fn col_fits(need: usize, w: usize) -> bool {
    need < w
}

pub fn truncate(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        s.to_string()
    } else if w <= 1 {
        "…".into()
    } else {
        let t: String = s.chars().take(w - 1).collect();
        format!("{t}…")
    }
}

// ------------------------------------------------------------------ graphs

/// Filled area graph with optional overlay series (SI style: total filled,
/// sub-series overlaid), newest sample at the right edge.
#[allow(clippy::too_many_arguments)]
pub fn area_graph(buf: &mut Buffer, x: usize, y: usize, w: usize, h: usize, total: &[f32], sub: &[f32], max: f32, c_total: i16, c_sub: i16) {
    if w == 0 || h == 0 || max <= 0.0 {
        return;
    }
    let draw = |buf: &mut Buffer, data: &[f32], color: i16| {
        for i in 0..w {
            let idx = data.len().saturating_sub(w) + i;
            if idx >= data.len() {
                break;
            }
            let v = data[idx].clamp(0.0, max);
            let rows = ((v / max) * h as f32).round() as usize;
            for r in 0..rows.min(h) {
                buf.set(x + i, y + h - 1 - r, '█', Style::new(color, -1));
            }
        }
    };
    draw(buf, total, c_total);
    if !sub.is_empty() {
        draw(buf, sub, c_sub);
    }
}

/// Labeled graph block: title, current-value line, graph body.
#[allow(clippy::too_many_arguments)]
pub fn graph_panel(buf: &mut Buffer, x: usize, y: usize, w: usize, h: usize, title: &str, subtitle: &str, total: &[f32], sub: &[f32], max: f32, c_total: i16, c_sub: i16) {
    if h < 4 {
        return;
    }
    buf.set_str(x, y, title, S_BOLD);
    buf.set_cell(x, y + 1, w, subtitle, S_DIM);
    area_graph(buf, x, y + 2, w, h - 2, total, sub, max, c_total, c_sub);
}

// ------------------------------------------------------------ chrome

pub fn menu_bar(buf: &mut Buffer, w: usize, title: &str, right: &str) {
    buf.fill_rect(0, 0, w, 1, S_HEADER);
    buf.set_str(1, 0, title, S_HEADER.bold());
    buf.set_str_right(w - 1, 0, right, S_HEADER);
}

pub fn toolbar(buf: &mut Buffer, w: usize, hints: &str, search_label: &str, search: &str, searching: bool) {
    buf.fill_rect(0, 1, w, 1, Style::new(-1, 235));
    buf.set_cell(1, 1, w / 2, hints, Style::new(C_ACCENT, 235));
    let box_w = 28usize;
    if w > box_w + 4 {
        let label = format!("{search_label} (Ctrl+K)");
        let bw = box_w.min(w / 3);
        let text_w = bw - 2;
        let shown = truncate(search, text_w);
        let s = if searching {
            Style::new(15, 28)
        } else {
            Style::new(C_DIM_FG, 235)
        };
        let x = w - bw - 1;
        let label_x = x.saturating_sub(label.chars().count() + 1);
        buf.set_str(label_x, 1, &label, Style::new(C_HEADER_FG, 235));
        buf.set(x, 1, '[', s);
        buf.set_cell(x + 1, 1, text_w, &shown, s);
        if searching {
            let cur = (shown.chars().count() + 1).min(text_w);
            buf.set(x + cur, 1, '▌', Style::new(15, 28));
        }
        buf.set(x + bw - 1, 1, ']', s);
    }
}

pub fn tabs(buf: &mut Buffer, w: usize, names: &[&str], active: usize) {
    buf.fill_rect(0, 2, w, 1, S_HEADER);
    let mut x = 1;
    for (i, name) in names.iter().enumerate() {
        let label = format!(" {name} ");
        let style = if i == active { Style::new(15, C_TAB_ACTIVE_BG).bold() } else { S_HEADER };
        buf.set_str(x, 2, &label, style);
        x += label.chars().count() + 1;
    }
    let hint = "Tab: switch  1-6: jump";
    buf.set_str_right(w - 1, 2, hint, S_HEADER.dim());
}

pub fn status_bar(buf: &mut Buffer, w: usize, h: usize, left: &str, right: &str) {
    buf.fill_rect(0, h - 1, w, 1, S_HEADER);
    buf.set_cell(1, h - 1, w / 2 + 10, left, S_HEADER);
    if !right.is_empty() {
        buf.set_str_right(w - 1, h - 1, right, S_HEADER);
    }
}

/// Centered modal dialog. `lines` = (text, style). Returns (x, y, w, h) used.
pub fn dialog(buf: &mut Buffer, title: &str, lines: &[(String, Style)], footer: &str) -> (usize, usize, usize, usize) {
    let content_w = lines.iter().map(|(l, _)| l.chars().count()).chain([title.chars().count() + 4, footer.chars().count() + 4]).max().unwrap_or(20);
    let w = (content_w + 6).min(buf.w.saturating_sub(4)).max(24);
    let h = (lines.len() + 5).min(buf.h.saturating_sub(2));
    let x = buf.w.saturating_sub(w) / 2;
    let y = buf.h.saturating_sub(h) / 2;
    // dim backdrop is not possible per-cell cheaply — just draw an opaque box
    buf.fill_rect(x, y, w, h, Style::new(-1, 17));
    buf.draw_box(x, y, w, h, Style::new(C_ACCENT, 17));
    buf.set_str(x + 2, y, &format!(" {title} "), Style::new(15, 17).bold());
    for (i, (line, style)) in lines.iter().take(h.saturating_sub(5)).enumerate() {
        let st = Style { bg: 17, ..*style };
        buf.set_cell(x + 3, y + 2 + i, w - 6, line, st);
    }
    buf.set_cell(x + 3, y + h - 2, w - 6, footer, Style::new(C_DIM_FG, 17));
    (x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<Column> {
        vec![Column::flex("Name"), Column::right("PID", 7), Column::right("CPU", 7)]
    }

    #[test]
    fn table_navigation_and_sort() {
        let mut t = Table::new(cols(), 1);
        t.move_sel(5, 10);
        assert_eq!(t.selected, 5);
        t.move_sel(-99, 10);
        assert_eq!(t.selected, 0);
        t.move_sel(99, 10);
        assert_eq!(t.selected, 9);
        t.sort_by(2);
        assert_eq!(t.sort_col, 2);
        t.sort_by(2);
        assert!(!t.sort_asc);
    }

    #[test]
    fn render_rows_with_tree() {
        let mut b = Buffer::new(60, 8);
        let t = Table::new(cols(), 0);
        let rows = vec![
            Row { id: 1, cells: vec!["launchd".into(), "1".into(), "0.1".into()], numeric: vec![], depth: 0, has_children: true, expanded: true, style: S_SYSTEM },
            Row { id: 2, cells: vec!["child".into(), "2".into(), "0.0".into()], numeric: vec![], depth: 1, has_children: false, expanded: false, style: S_NORMAL },
        ];
        render_table(&mut b, 0, 0, 60, 8, &t, &rows, true);
        let text = b.to_text();
        assert!(text.contains("Name ▲"));
        assert!(text.contains("▾ launchd"));
        assert!(text.contains("  child"));
    }

    #[test]
    fn graph_draws_bars() {
        let mut b = Buffer::new(10, 6);
        let data = vec![50.0; 10];
        area_graph(&mut b, 0, 0, 10, 4, &data, &[], 100.0, C_CPU, 0);
        assert_eq!(b.cells[3 * 10 + 5].ch, '█'); // bottom row filled
        assert_eq!(b.cells[2 * 10 + 5].ch, '█'); // 50% of 4 rows = 2 rows → rows 2,3
        assert_eq!(b.cells[10 + 5].ch, ' ');
        assert_eq!(b.cells[5].ch, ' ');
    }
}
