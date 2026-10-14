//! Virtualized grid: only the rows and columns in view are laid out and
//! painted, so sheets with hundreds of thousands of rows scroll smoothly.

use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Id, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2,
};

use super::app::{App, CELL_EDITOR_ID};
use super::selection::Selection;
use super::util::set_cursor_end;
use crate::model::{DEFAULT_COL_WIDTH, HAlign, Value, refs};

pub const ROW_H: f32 = 22.0;
pub const HEADER_H: f32 = 22.0;
const RESIZE_GRAB: f32 = 4.0;
const MIN_COL_W: f32 = 24.0;

#[derive(Clone, Copy, Debug)]
enum Hit {
    Corner,
    ColHeader(u32),
    ColResize(u32),
    FilterButton(u32),
    RowHeader(u32),
    Cell(u32, u32),
}

pub(super) struct Layout {
    origin: Pos2,
    viewport: Rect,
    pub(super) row_header_w: f32,
    /// xs[c] = left edge of column c (content coords, excluding the row header).
    pub(super) xs: Vec<f32>,
    n_view_rows: u32,
    /// Frozen view rows and columns, pinned to the top/left while scrolling.
    frozen_rows: u32,
    frozen_cols: u32,
}

impl Layout {
    pub(super) fn col_at(&self, x: f32) -> u32 {
        match self.xs.binary_search_by(|v| v.partial_cmp(&x).unwrap()) {
            Ok(i) => i as u32,
            Err(i) => i.saturating_sub(1) as u32,
        }
        .min(self.xs.len().saturating_sub(2) as u32)
    }

    pub(super) fn frozen_w(&self) -> f32 {
        self.xs[(self.frozen_cols as usize).min(self.xs.len() - 1)]
    }

    pub(super) fn frozen_h(&self) -> f32 {
        self.frozen_rows as f32 * ROW_H
    }

    /// Frozen columns ignore horizontal scrolling, frozen rows vertical.
    pub(super) fn x_origin(&self, c: u32) -> f32 {
        if c < self.frozen_cols {
            self.left()
        } else {
            self.origin.x
        }
    }

    pub(super) fn y_origin(&self, vi: u32) -> f32 {
        if vi < self.frozen_rows {
            self.top()
        } else {
            self.origin.y
        }
    }

    /// Screen rect of a cell in view row `vi`, column `c`.
    pub(super) fn cell_rect(&self, vi: u32, c: u32) -> Rect {
        let x = self.x_origin(c) + self.row_header_w;
        let y0 = self.y_origin(vi) + HEADER_H + vi as f32 * ROW_H;
        Rect::from_min_max(
            pos2(x + self.xs[c as usize], y0),
            pos2(x + self.xs[c as usize + 1], y0 + ROW_H),
        )
    }

    pub(super) fn top(&self) -> f32 {
        self.origin.y + self.viewport.min.y
    }

    pub(super) fn left(&self) -> f32 {
        self.origin.x + self.viewport.min.x
    }

    /// Where the scrolling part of the grid starts on screen.
    pub(super) fn scroll_left(&self) -> f32 {
        self.left() + self.row_header_w + self.frozen_w()
    }

    pub(super) fn scroll_top(&self) -> f32 {
        self.top() + HEADER_H + self.frozen_h()
    }

    /// The screen area of a region: frozen or scrolling rows × columns.
    pub(super) fn region_clip(&self, visible: Rect, frozen_row: bool, frozen_col: bool) -> Rect {
        let (x0, x1) = if frozen_col {
            (self.left() + self.row_header_w, self.scroll_left())
        } else {
            (self.scroll_left(), visible.max.x)
        };
        let (y0, y1) = if frozen_row {
            (self.top() + HEADER_H, self.scroll_top())
        } else {
            (self.scroll_top(), visible.max.y)
        };
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)).intersect(visible)
    }

    /// The view row under screen `y` (inside the cell area).
    pub(super) fn row_at_y(&self, y: f32) -> u32 {
        let base = if y < self.scroll_top() {
            self.top()
        } else {
            self.origin.y
        };
        (((y - base - HEADER_H) / ROW_H).floor().max(0.0) as u32)
            .min(self.n_view_rows.saturating_sub(1))
    }

    /// The column under screen `x` (right of the row header).
    pub(super) fn col_at_x(&self, x: f32) -> u32 {
        let base = if x < self.scroll_left() {
            self.left()
        } else {
            self.origin.x
        };
        self.col_at(x - base - self.row_header_w)
    }
}

impl App {
    fn col_width(&self, c: u32) -> f32 {
        self.sheet()
            .col_widths
            .get(&c)
            .copied()
            .unwrap_or(DEFAULT_COL_WIDTH)
    }

    pub fn grid_ui(&mut self, ui: &mut egui::Ui) {
        let n_cols = self.view_cols();
        let mut xs = Vec::with_capacity(n_cols as usize + 1);
        let mut x = 0.0;
        xs.push(0.0);
        for c in 0..n_cols {
            x += self.col_width(c);
            xs.push(x);
        }
        let n_view_rows = self.view_rows();
        let last_row_label = self.row_at(n_view_rows.saturating_sub(1)).unwrap_or(0) + 1;
        let row_header_w = (last_row_label.to_string().len() as f32 * 8.0 + 18.0).max(44.0);
        let content = vec2(row_header_w + x, HEADER_H + n_view_rows as f32 * ROW_H);

        let mut scroll = egui::ScrollArea::both()
            .id_salt(("grid", self.doc_id(), self.active))
            .auto_shrink(false)
            .scroll_source(
                egui::scroll_area::ScrollSource::MOUSE_WHEEL
                    | egui::scroll_area::ScrollSource::SCROLL_BAR,
            );
        if self.scroll_to_cursor {
            // Handled inside via scroll_to_rect; nothing to configure here.
            scroll = scroll.animated(false);
        }
        scroll.show_viewport(ui, |ui, viewport| {
            ui.set_min_size(content);
            let freeze = self.sheet().freeze;
            // Frozen sheet rows → view rows (filters can hide some of them).
            let frozen_rows = match &self.view().rows {
                Some(rows) => rows.partition_point(|&r| r < freeze.rows) as u32,
                None => freeze.rows,
            };
            let frozen_cols = freeze.cols.min(xs.len() as u32 - 1);
            let layout = Layout {
                origin: ui.max_rect().min,
                viewport,
                row_header_w,
                xs,
                n_view_rows,
                frozen_rows,
                frozen_cols,
            };
            self.grid_contents(ui, &layout);
        });
    }

    fn hit_test(&self, l: &Layout, p: Pos2) -> Hit {
        let in_header = p.y < l.top() + HEADER_H;
        let in_rowhdr = p.x < l.left() + l.row_header_w;
        let col = l.col_at_x(p.x);
        let row = self.row_at(l.row_at_y(p.y)).unwrap_or(0);
        match (in_header, in_rowhdr) {
            (true, true) => Hit::Corner,
            (true, false) => {
                let rect = l.cell_rect(0, col);
                if (p.x - rect.right()).abs() <= RESIZE_GRAB {
                    Hit::ColResize(col)
                } else if col > 0 && (p.x - rect.left()).abs() <= RESIZE_GRAB {
                    Hit::ColResize(col - 1)
                } else if self.filter_buttons && rect.right() - p.x < 18.0 {
                    Hit::FilterButton(col)
                } else {
                    Hit::ColHeader(col)
                }
            }
            (false, true) => Hit::RowHeader(row),
            (false, false) => Hit::Cell(row, col),
        }
    }

    fn grid_contents(&mut self, ui: &mut egui::Ui, l: &Layout) {
        let ctx = ui.ctx().clone();
        let visible = Rect::from_min_size(l.origin + l.viewport.min.to_vec2(), l.viewport.size());
        let resp = ui.interact(visible, Id::new("grid_area"), Sense::click_and_drag());
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        let shift = ctx.input(|i| i.modifiers.shift);
        let max_row = self.sheet().n_rows.max(1) - 1;

        // ---------- hover cursors ----------
        if let Some(p) = resp.hover_pos() {
            match self.hit_test(l, p) {
                Hit::ColResize(_) => ctx.set_cursor_icon(CursorIcon::ResizeHorizontal),
                Hit::Cell(..) => ctx.set_cursor_icon(CursorIcon::Cell),
                _ => {}
            }
        }
        if self.resizing.is_some() {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
        }

        // ---------- mouse press ----------
        if resp.is_pointer_button_down_on()
            && ctx.input(|i| i.pointer.primary_pressed())
            && let Some(p) = pointer
        {
            self.selected_chart = None;
            let hit = self.hit_test(l, p);
            let pointing = self.wants_reference();
            match hit {
                Hit::ColResize(c) => self.resizing = Some((c, self.col_width(c), p.x)),
                Hit::FilterButton(c) => {
                    self.commit_edit((0, 0));
                    self.open_filter_popup(c);
                    if let Some(popup) = self.filter_popup.as_mut() {
                        popup.anchor = Some(pos2(p.x - 150.0, l.top() + HEADER_H + 4.0));
                    }
                }
                Hit::Cell(r, c)
                    if pointing && self.edit.as_ref().is_some_and(|e| e.pos != (r, c)) =>
                {
                    self.point_at((r, c), true);
                    let id = if self.edit.as_ref().unwrap().in_bar {
                        super::app::BAR_EDITOR_ID
                    } else {
                        CELL_EDITOR_ID
                    };
                    ctx.memory_mut(|m| m.request_focus(Id::new(id)));
                }
                Hit::Cell(r, c) => {
                    if self.edit.as_ref().is_some_and(|e| e.pos != (r, c)) {
                        self.commit_edit((0, 0));
                    }
                    if self.edit.is_none() {
                        ctx.memory_mut(|m| m.stop_text_input());
                        self.set_cursor_mouse((r, c), shift);
                    }
                }
                Hit::ColHeader(c) => {
                    self.commit_edit((0, 0));
                    let anchor = if shift { self.sel.anchor } else { (0, c) };
                    self.sel = Selection {
                        anchor: (0, anchor.1),
                        cursor: (max_row, c),
                    };
                }
                Hit::RowHeader(r) => {
                    self.commit_edit((0, 0));
                    let last_col = self.sheet().n_cols.max(1) - 1;
                    let anchor = if shift { self.sel.anchor.0 } else { r };
                    self.sel = Selection {
                        anchor: (anchor, 0),
                        cursor: (r, last_col),
                    };
                }
                Hit::Corner => {
                    self.commit_edit((0, 0));
                    let last_col = self.sheet().n_cols.max(1) - 1;
                    self.sel = Selection {
                        anchor: (0, 0),
                        cursor: (max_row, last_col),
                    };
                }
            }
        }

        // ---------- dragging ----------
        if resp.dragged()
            && let Some(p) = pointer
        {
            if let Some((c, start_w, start_x)) = self.resizing {
                let w = (start_w + p.x - start_x).max(MIN_COL_W);
                self.book.sheets[self.active].col_widths.insert(c, w);
                self.book.dirty = true;
            } else {
                // Auto-scroll when dragging past the edge.
                // (The frozen bands never scroll, so only the scrolling area counts.)
                let inner = Rect::from_min_max(pos2(l.scroll_left(), l.scroll_top()), visible.max);
                let cells = Rect::from_min_max(
                    pos2(l.left() + l.row_header_w, l.top() + HEADER_H),
                    visible.max,
                );
                let mut delta = Vec2::ZERO;
                if p.y > inner.max.y {
                    delta.y = -(p.y - inner.max.y).min(40.0);
                } else if p.y < inner.min.y {
                    delta.y = (inner.min.y - p.y).min(40.0);
                }
                if p.x > inner.max.x {
                    delta.x = -(p.x - inner.max.x).min(40.0);
                } else if p.x < inner.min.x {
                    delta.x = (inner.min.x - p.x).min(40.0);
                }
                if delta != Vec2::ZERO {
                    ui.scroll_with_delta(delta);
                    ctx.request_repaint();
                }
                let clamped = p.clamp(cells.min, cells.max - vec2(1.0, 1.0));
                match self.hit_test(l, clamped) {
                    Hit::Cell(r, c) => {
                        if self.edit.as_ref().is_some_and(|e| e.pointing.is_some()) {
                            self.point_at((r, c), false);
                        } else if self.edit.is_none() && self.sel.cursor != (r, c) {
                            self.sel.cursor = (r, c);
                            self.snap_to_merges();
                        }
                    }
                    Hit::ColHeader(c) | Hit::FilterButton(c)
                        if self.sel.anchor.0 == 0 && self.sel.cursor.0 == max_row =>
                    {
                        self.sel.cursor.1 = c;
                    }
                    Hit::RowHeader(r) if self.sel.anchor.1 == 0 => {
                        self.sel.cursor.0 = r;
                    }
                    _ => {}
                }
            }
        }
        if resp.drag_stopped() || !ctx.input(|i| i.pointer.primary_down()) {
            self.resizing = None;
        }

        // ---------- double clicks ----------
        if resp.double_clicked()
            && let Some(p) = pointer
        {
            match self.hit_test(l, p) {
                Hit::Cell(r, c) if self.edit.is_none() => {
                    self.sel = Selection::single((r, c));
                    self.start_edit(&ctx, None, false);
                }
                Hit::ColResize(c) => self.autofit(ui, c),
                _ => {}
            }
        }

        // ---------- context menu ----------
        if resp.secondary_clicked()
            && let Some(p) = pointer
        {
            self.context_target = match self.hit_test(l, p) {
                // Keep the selection only if it already spans whole columns/rows
                // that include the clicked header; otherwise select that one.
                Hit::ColHeader(c) | Hit::FilterButton(c) | Hit::ColResize(c) => {
                    let ((r0, c0), (r1, c1)) = self.sel.range();
                    let whole_cols = r0 == 0 && r1 >= max_row;
                    if !(whole_cols && (c0..=c1).contains(&c)) {
                        self.sel = Selection {
                            anchor: (0, c),
                            cursor: (max_row, c),
                        };
                    }
                    Some((Some(c), None))
                }
                Hit::RowHeader(r) => {
                    let ((r0, c0), (r1, c1)) = self.sel.range();
                    let last_col = self.sheet().n_cols.max(1) - 1;
                    let whole_rows = c0 == 0 && c1 >= last_col;
                    if !(whole_rows && (r0..=r1).contains(&r)) {
                        self.sel = Selection {
                            anchor: (r, 0),
                            cursor: (r, last_col),
                        };
                    }
                    Some((None, Some(r)))
                }
                Hit::Cell(r, c) => {
                    if !self.sel.contains((r, c)) {
                        self.sel = Selection::single((r, c));
                    }
                    Some((Some(c), Some(r)))
                }
                Hit::Corner => None,
            };
        }
        resp.context_menu(|ui| self.context_menu(ui));

        // ---------- keep the cursor in view ----------
        if self.scroll_to_cursor {
            self.scroll_to_cursor = false;
            let (r, c) = self.sel.cursor;
            if let Some(vi) = self.view_index(r)
                && (c as usize) + 1 < l.xs.len()
                && vi < l.n_view_rows
            {
                // Leave room for the headers and frozen bands; a frozen row or
                // column needs no scrolling in that direction.
                let rect = l.cell_rect(vi, c);
                let x = if c < l.frozen_cols {
                    visible.x_range()
                } else {
                    (rect.min.x - l.row_header_w - l.frozen_w()..=rect.max.x).into()
                };
                let y = if vi < l.frozen_rows {
                    visible.y_range()
                } else {
                    (rect.min.y - HEADER_H - l.frozen_h()..=rect.max.y).into()
                };
                ui.scroll_to_rect(Rect::from_x_y_ranges(x, y), None);
            }
        }

        self.paint(ui, l, visible);
        self.chart_layer(ui, l, visible);
        self.cell_editor(ui, l);
        self.paint_headers(ui, l, visible);
    }

    fn set_cursor_mouse(&mut self, pos: (u32, u32), extend: bool) {
        if extend {
            self.sel.cursor = pos;
        } else {
            self.sel = Selection::single(pos);
        }
        self.snap_to_merges();
    }

    fn visible_range(&self, l: &Layout) -> (u32, u32, u32, u32) {
        let first_vi = ((l.viewport.min.y - HEADER_H).max(0.0) / ROW_H).floor() as u32;
        let last_vi =
            (((l.viewport.max.y - HEADER_H) / ROW_H).ceil() as u32 + 1).min(l.n_view_rows);
        let first_c = l.col_at((l.viewport.min.x - l.row_header_w).max(0.0));
        let last_c = (l.col_at(l.viewport.max.x - l.row_header_w) + 1).min(l.xs.len() as u32 - 1);
        (first_vi, last_vi, first_c, last_c)
    }

    fn paint(&self, ui: &egui::Ui, l: &Layout, visible: Rect) {
        let painter = ui.painter().with_clip_rect(visible);
        let v = ui.visuals();
        let dark = v.dark_mode;
        let bg = if dark {
            Color32::from_gray(24)
        } else {
            Color32::WHITE
        };
        let grid = if dark {
            Color32::from_gray(52)
        } else {
            Color32::from_gray(222)
        };
        let text = v.text_color();
        let error = v.error_fg_color;
        let accent = v.selection.stroke.color;
        let sel_fill = v.selection.bg_fill.gamma_multiply(0.35);
        let match_fill = Color32::from_rgba_unmultiplied(255, 210, 0, if dark { 60 } else { 90 });
        let current_match_fill = Color32::from_rgba_unmultiplied(255, 150, 0, 140);
        let font = FontId::proportional(13.0);

        painter.rect_filled(visible, 0.0, bg);

        let (first_vi, last_vi, first_c, last_c) = self.visible_range(l);
        let sheet = self.sheet();
        let view = self.view();
        let current = view.matches.get(view.current_match).copied();
        let has_search = !view.matches.is_empty();
        let editing = self.edit.as_ref().map(|e| e.pos);

        let fr = l.frozen_rows.min(l.n_view_rows);
        let fc = l.frozen_cols;
        // (view rows, columns, frozen row?, frozen column?) for each region,
        // scrolling first so the pinned bands paint over it.
        let scroll_rows = first_vi.max(fr)..last_vi;
        let scroll_cols = first_c.max(fc)..last_c;
        let regions = [
            (scroll_rows.clone(), scroll_cols.clone(), false, false),
            (0..fr, scroll_cols, true, false),
            (scroll_rows, 0..fc, false, true),
            (0..fr, 0..fc, true, true),
        ];

        let stroke = Stroke::new(1.0, grid);
        let border_stroke = Stroke::new(1.0, v.strong_text_color());
        // Paint one cell, or a merged range as one cell (`merged` covers the
        // inner grid lines and highlights the whole range).
        let paint_cell = |painter: &egui::Painter,
                          rect: Rect,
                          pos: (u32, u32),
                          merged: Option<crate::model::Merge>| {
            let cell = sheet.cells.get(&pos);
            let style = cell.map(|c| c.style).unwrap_or_default();
            if merged.is_some() {
                painter.rect_filled(rect.shrink(0.5), 0.0, bg);
            }
            if let Some(fill) = style.fill {
                painter.rect_filled(rect, 0.0, rgb(fill));
            }
            let selected = match merged {
                Some(m) => crate::model::merges_overlap(m, self.sel.range()),
                None => self.sel.contains(pos),
            };
            if selected && !self.sel_is_single_cell() {
                painter.rect_filled(rect, 0.0, sel_fill);
            }
            if has_search && Some(pos) == current {
                painter.rect_filled(rect, 0.0, current_match_fill);
            } else if has_search && view.matches.binary_search(&pos).is_ok() {
                painter.rect_filled(rect, 0.0, match_fill);
            }
            if style.borders.any() {
                paint_borders(painter, rect, style.borders, border_stroke);
            }
            let value = sheet.value(pos);
            if editing == Some(pos) || matches!(value, Value::Empty) {
                return;
            }
            // Explicit colour, else readable on the fill, else the theme's.
            let color = match (style.color, style.fill, value) {
                (Some(c), _, _) => rgb(c),
                (None, _, Value::Error(_)) => error,
                (None, Some(fill), _) if fill.luminance() > 0.4 => Color32::from_gray(20),
                (None, Some(_), _) => Color32::from_gray(240),
                (None, None, _) => text,
            };
            let galley =
                |s: String| ui.fonts_mut(|f| f.layout_job(text_job(s, &font, color, &style)));
            let mut g = galley(sheet.display(pos));
            // Numbers that don't fit show as ### like Excel.
            if matches!(value, Value::Number(_)) && g.size().x > rect.width() - 6.0 {
                g = galley("#".repeat(((rect.width() - 6.0) / 7.0).max(1.0) as usize));
            }
            let align = match (style.align, value) {
                (HAlign::Left, _) => Align::Min,
                (HAlign::Center, _) => Align::Center,
                (HAlign::Right, _) => Align::Max,
                (HAlign::General, Value::Number(_)) => Align::Max,
                (HAlign::General, Value::Bool(_) | Value::Error(_)) => Align::Center,
                (HAlign::General, _) => Align::Min,
            };
            let x = match align {
                Align::Min => rect.left() + 4.0,
                Align::Center => rect.center().x - g.size().x / 2.0,
                Align::Max => rect.right() - 4.0 - g.size().x,
            };
            let at = pos2(x, rect.center().y - g.size().y / 2.0);
            let clipped =
                painter.with_clip_rect(rect.shrink2(vec2(1.0, 0.0)).intersect(painter.clip_rect()));
            clipped.galley(at, g.clone(), color);
            if style.bold {
                // The bundled font has no bold weight: strike twice, slightly offset.
                clipped.galley(at + vec2(0.55, 0.0), g, color);
            }
        };

        let all_merges = self.visible_merges(l, first_vi, last_vi, first_c, last_c);
        for (rows, cols, frozen_row, frozen_col) in regions.iter().cloned() {
            if rows.is_empty() || cols.is_empty() {
                continue;
            }
            let clip = l.region_clip(visible, frozen_row, frozen_col);
            let painter = ui.painter().with_clip_rect(clip);
            // Grid lines.
            for vi in rows.start..=rows.end {
                let y = l
                    .cell_rect(vi.min(l.n_view_rows.saturating_sub(1)), cols.start)
                    .top()
                    + if vi >= l.n_view_rows { ROW_H } else { 0.0 };
                painter.hline(clip.x_range(), y - 0.5, stroke);
            }
            for c in cols.start..=cols.end {
                let x = l.cell_rect(rows.start, c.min(l.xs.len() as u32 - 2)).left()
                    + if c as usize >= l.xs.len() - 1 {
                        l.cell_rect(rows.start, c - 1).width()
                    } else {
                        0.0
                    };
                painter.vline(x - 0.5, clip.y_range(), stroke);
            }
            for vi in rows.clone() {
                let Some(r) = self.row_at(vi) else { continue };
                for c in cols.clone() {
                    if all_merges
                        .iter()
                        .any(|(m, _)| crate::model::merge_contains(*m, (r, c)))
                    {
                        continue;
                    }
                    paint_cell(&painter, l.cell_rect(vi, c), (r, c), None);
                }
            }
            // A merge is drawn by the region holding its top-left cell.
            for &(m, rect) in &all_merges {
                let in_region = self.view_index(m.0.0).is_some_and(|vi| rows.contains(&vi))
                    && cols.contains(&m.0.1);
                if in_region {
                    paint_cell(&painter, rect, m.0, Some(m));
                }
            }
        }

        let paint_overlays = |painter: &egui::Painter| {
            // Selection outline and cursor.
            let ((r0, c0), (r1, c1)) = self.sel.range();
            if let (Some(v0), Some(v1)) = (
                self.first_visible_at_or_after(r0),
                self.last_visible_at_or_before(r1),
            ) && v0 <= v1
                && (c1 as usize) + 1 < l.xs.len()
            {
                let outline = l.cell_rect(v0, c0).union(l.cell_rect(v1, c1));
                painter.rect_stroke(
                    outline,
                    0.0,
                    Stroke::new(2.0, accent),
                    egui::StrokeKind::Inside,
                );
            }
            let cursor_rect = match self.sheet().merge_at(self.sel.cursor) {
                Some(m) => self.merge_rect(l, m),
                None => self
                    .view_index(self.sel.cursor.0)
                    .filter(|_| (self.sel.cursor.1 as usize) + 1 < l.xs.len())
                    .map(|vi| l.cell_rect(vi, self.sel.cursor.1)),
            };
            if let Some(rect) = cursor_rect {
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(2.0, accent),
                    egui::StrokeKind::Inside,
                );
            }

            // Formula reference being pointed at.
            if let Some((anchor, _)) = self.edit.as_ref().and_then(|e| e.pointing)
                && let Some(cursor) = self.pointing_cursor()
            {
                let s = Selection { anchor, cursor };
                let ((r0, c0), (r1, c1)) = s.range();
                if let (Some(v0), Some(v1)) = (self.view_index(r0), self.view_index(r1)) {
                    let rect = l.cell_rect(v0, c0).union(l.cell_rect(v1, c1));
                    let blue = Color32::from_rgb(40, 120, 240);
                    painter.rect_filled(rect, 0.0, blue.gamma_multiply(0.12));
                    painter.rect_stroke(
                        rect,
                        0.0,
                        Stroke::new(1.5, blue),
                        egui::StrokeKind::Inside,
                    );
                }
            }

            // Marching outline around copied/cut range.
            if let Some(sel) = self.clip_outline() {
                let ((r0, c0), (r1, c1)) = sel.range();
                if let (Some(v0), Some(v1)) = (self.view_index(r0), self.view_index(r1))
                    && (c1 as usize) + 1 < l.xs.len()
                {
                    let rect = l.cell_rect(v0, c0).union(l.cell_rect(v1, c1));
                    painter.rect_stroke(
                        rect.shrink(1.0),
                        0.0,
                        Stroke::new(1.0, accent),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        };
        for (_, _, frozen_row, frozen_col) in regions.iter().cloned() {
            paint_overlays(
                &ui.painter()
                    .with_clip_rect(l.region_clip(visible, frozen_row, frozen_col)),
            );
        }

        // Freeze lines.
        let divider = Stroke::new(1.5, v.strong_text_color().gamma_multiply(0.6));
        if fc > 0 {
            painter.vline(
                l.scroll_left() - 0.5,
                (l.top() + HEADER_H)..=visible.max.y,
                divider,
            );
        }
        if fr > 0 {
            painter.hline(
                (l.left() + l.row_header_w)..=visible.max.x,
                l.scroll_top() - 0.5,
                divider,
            );
        }
    }

    /// Merges overlapping the visible area, with their on-screen rects.
    fn visible_merges(
        &self,
        l: &Layout,
        first_vi: u32,
        last_vi: u32,
        first_c: u32,
        last_c: u32,
    ) -> Vec<(crate::model::Merge, Rect)> {
        let first_r = self.row_at(first_vi).unwrap_or(0);
        let last_r = self.row_at(last_vi.saturating_sub(1)).unwrap_or(first_r);
        self.sheet()
            .merges
            .iter()
            .copied()
            .filter(|&((r0, c0), (r1, c1))| {
                r1 >= first_r && r0 <= last_r && c1 >= first_c && c0 < last_c
            })
            .filter_map(|m| Some((m, self.merge_rect(l, m)?)))
            .collect()
    }

    /// Screen rect of a merged range (its visible rows, when filtered).
    fn merge_rect(&self, l: &Layout, ((r0, c0), (r1, c1)): crate::model::Merge) -> Option<Rect> {
        let v0 = self.first_visible_at_or_after(r0)?;
        let v1 = self.last_visible_at_or_before(r1)?;
        if v1 < v0 {
            return None;
        }
        let c1 = c1.min(l.xs.len() as u32 - 2);
        Some(l.cell_rect(v0, c0).union(l.cell_rect(v1, c1)))
    }

    /// True when the selection is one cell or exactly one merged range.
    pub(crate) fn sel_is_single_cell(&self) -> bool {
        self.sel.is_single() || self.sheet().merge_at(self.sel.cursor) == Some(self.sel.range())
    }

    fn first_visible_at_or_after(&self, row: u32) -> Option<u32> {
        match &self.view().rows {
            None => Some(row),
            Some(rows) => {
                let i = rows.partition_point(|&r| r < row);
                (i < rows.len()).then_some(i as u32)
            }
        }
    }

    fn last_visible_at_or_before(&self, row: u32) -> Option<u32> {
        match &self.view().rows {
            None => Some(row),
            Some(rows) => {
                let i = rows.partition_point(|&r| r <= row);
                (i > 0).then(|| i as u32 - 1)
            }
        }
    }

    fn pointing_cursor(&self) -> Option<(u32, u32)> {
        let e = self.edit.as_ref()?;
        let (_, len) = e.pointing?;
        let tail = e.buf.get(len..)?;
        let last = tail.split(':').next_back()?;
        refs::parse_cell(last)
    }

    fn paint_headers(&self, ui: &egui::Ui, l: &Layout, visible: Rect) {
        let painter = ui.painter().with_clip_rect(visible);
        let v = ui.visuals();
        let dark = v.dark_mode;
        let hdr_bg = if dark {
            Color32::from_gray(36)
        } else {
            Color32::from_gray(245)
        };
        let hdr_sel = v
            .selection
            .bg_fill
            .gamma_multiply(if dark { 0.6 } else { 0.35 });
        let line = if dark {
            Color32::from_gray(60)
        } else {
            Color32::from_gray(210)
        };
        let text = v.weak_text_color();
        let strong = v.strong_text_color();
        let accent = v.selection.stroke.color;
        let font = FontId::proportional(12.0);
        let (first_vi, last_vi, first_c, last_c) = self.visible_range(l);
        let ((sr0, sc0), (sr1, sc1)) = self.sel.range();
        let view = self.view();

        // Column header strip (frozen columns pinned).
        let top = l.top();
        let left_edge = l.left() + l.row_header_w;
        let strip = Rect::from_min_size(pos2(visible.min.x, top), vec2(visible.width(), HEADER_H));
        painter.rect_filled(strip, 0.0, hdr_bg);
        let fc = l.frozen_cols;
        let hdr_clip = |x0: f32, x1: f32| {
            Rect::from_x_y_ranges(x0..=x1, top..=top + HEADER_H).intersect(visible)
        };
        let col_parts = [
            (
                first_c.max(fc)..last_c,
                hdr_clip(l.scroll_left(), visible.max.x),
            ),
            (0..fc, hdr_clip(left_edge, l.scroll_left())),
        ];
        for (cols, clip) in col_parts {
            let painter = painter.with_clip_rect(clip);
            for c in cols {
                let x = l.cell_rect(0, c).x_range();
                let (x0, x1) = (x.min, x.max);
                let rect = Rect::from_min_max(pos2(x0, top), pos2(x1, top + HEADER_H));
                let selected = (sc0..=sc1).contains(&c);
                if selected {
                    painter.rect_filled(rect, 0.0, hdr_sel);
                }
                painter.vline(x1 - 0.5, rect.y_range(), Stroke::new(1.0, line));
                let filtered = view.filters.get(&c).is_some_and(|f| f.is_active());
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    refs::col_name(c),
                    font.clone(),
                    if selected { strong } else { text },
                );
                if self.filter_buttons || filtered {
                    let btn = Rect::from_min_max(
                        pos2(x1 - 16.0, top + 3.0),
                        pos2(x1 - 3.0, top + HEADER_H - 3.0),
                    );
                    let col = if filtered { accent } else { text };
                    if filtered {
                        painter.rect_filled(btn, 2.0, accent.gamma_multiply(0.2));
                    }
                    painter.text(btn.center(), Align2::CENTER_CENTER, "⏷", font.clone(), col);
                }
            }
        }

        painter.hline(
            visible.x_range(),
            top + HEADER_H - 0.5,
            Stroke::new(1.0, line),
        );

        // Row header strip.
        let left = l.left();
        let strip = Rect::from_min_size(
            pos2(left, visible.min.y),
            vec2(l.row_header_w, visible.height()),
        );
        painter.rect_filled(strip, 0.0, hdr_bg);
        let fr = l.frozen_rows.min(l.n_view_rows);
        let row_clip = |y0: f32, y1: f32| {
            Rect::from_x_y_ranges(left..=left + l.row_header_w, y0..=y1).intersect(visible)
        };
        let row_parts = [
            (
                first_vi.max(fr)..last_vi,
                row_clip(l.scroll_top(), visible.max.y),
            ),
            (0..fr, row_clip(top + HEADER_H, l.scroll_top())),
        ];
        for (rows, clip) in row_parts {
            let painter = painter.with_clip_rect(clip);
            for vi in rows {
                let Some(r) = self.row_at(vi) else { continue };
                let y0 = l.cell_rect(vi, 0).top();
                let rect =
                    Rect::from_min_max(pos2(left, y0), pos2(left + l.row_header_w, y0 + ROW_H));
                let selected = (sr0..=sr1).contains(&r);
                if selected {
                    painter.rect_filled(rect, 0.0, hdr_sel);
                }
                painter.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, line));
                // Rows whose numbers skip (filtered) are shown in the accent colour, like Excel.
                let color = if view.is_filtered() {
                    accent
                } else if selected {
                    strong
                } else {
                    text
                };
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    (r + 1).to_string(),
                    font.clone(),
                    color,
                );
            }
        }

        painter.vline(
            left + l.row_header_w - 0.5,
            visible.y_range(),
            Stroke::new(1.0, line),
        );

        // Corner.
        let corner = Rect::from_min_size(pos2(left, top), vec2(l.row_header_w, HEADER_H));
        painter.rect_filled(corner, 0.0, hdr_bg);
        painter.rect_stroke(
            corner,
            0.0,
            Stroke::new(1.0, line),
            egui::StrokeKind::Inside,
        );
    }

    fn cell_editor(&mut self, ui: &mut egui::Ui, l: &Layout) {
        let Some(pos) = self.edit.as_ref().map(|e| e.pos) else {
            return;
        };
        // A merged cell is edited across its whole range.
        let merged = self
            .sheet()
            .merge_at(pos)
            .and_then(|m| self.merge_rect(l, m));
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        let (r, c) = edit.pos;
        let vi = match &self.views[self.active].rows {
            Some(rows) => match rows.binary_search(&r) {
                Ok(i) => i as u32,
                Err(_) => return,
            },
            None => r,
        };
        if (c as usize) + 1 >= l.xs.len() {
            return;
        }
        let rect = merged.unwrap_or_else(|| l.cell_rect(vi, c));
        // Grow the editor to fit the text.
        let font = FontId::proportional(13.0);
        let text_w = ui.fonts_mut(|f| {
            f.layout_no_wrap(edit.buf.clone(), font.clone(), Color32::WHITE)
                .size()
                .x
        });
        let rect = Rect::from_min_size(
            rect.min,
            vec2(rect.width().max(text_w + 16.0), rect.height()),
        );
        let bg = if ui.visuals().dark_mode {
            Color32::from_gray(24)
        } else {
            Color32::WHITE
        };
        ui.painter().rect_filled(rect, 0.0, bg);
        let id = Id::new(CELL_EDITOR_ID);
        let resp = ui.put(
            rect.shrink2(vec2(2.0, 1.0)),
            egui::TextEdit::singleline(&mut edit.buf)
                .id(id)
                .frame(egui::Frame::NONE)
                .font(font)
                .margin(vec2(2.0, 3.0))
                .desired_width(rect.width() - 4.0),
        );
        let accent = ui.visuals().selection.stroke.color;
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(2.0, accent),
            egui::StrokeKind::Inside,
        );
        if !edit.in_bar && edit.place_cursor && resp.has_focus() {
            set_cursor_end(ui.ctx(), id, &edit.buf);
            edit.place_cursor = false;
        }
        if resp.gained_focus() {
            edit.in_bar = false;
        }
        if resp.changed() {
            edit.pointing = None;
        }
        // Clicking cells to build a formula drops keyboard focus; take it back.
        if !edit.in_bar && edit.pointing.is_some() && ui.ctx().memory(|m| m.focused().is_none()) {
            resp.request_focus();
        }
    }

    fn autofit(&mut self, ui: &egui::Ui, c: u32) {
        let font = FontId::proportional(13.0);
        let sheet = self.sheet();
        let mut w: f32 = 40.0;
        for r in 0..sheet.n_rows.min(5000) {
            let s = sheet.display((r, c));
            if !s.is_empty() {
                let tw =
                    ui.fonts_mut(|f| f.layout_no_wrap(s, font.clone(), Color32::WHITE).size().x);
                w = w.max(tw + 14.0);
            }
        }
        self.book.sheets[self.active]
            .col_widths
            .insert(c, w.min(600.0));
        self.book.dirty = true;
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let (col, row) = self.context_target.unwrap_or((None, None));
        if row.is_some() {
            if ui.button("Cut").clicked() {
                self.copy(&ctx, true);
                ui.close();
            }
            if ui.button("Copy").clicked() {
                self.copy(&ctx, false);
                ui.close();
            }
            if ui.button("Clear contents").clicked() {
                self.clear_selection();
                ui.close();
            }
            ui.separator();
        }
        self.structural_menu(ui, col.is_some(), row.is_some());
        if let Some(c) = col {
            if ui.button("Sort A to Z").clicked() {
                self.sort_by(c, true);
                ui.close();
            }
            if ui.button("Sort Z to A").clicked() {
                self.sort_by(c, false);
                ui.close();
            }
            ui.separator();
            if ui.button("Filter this column…").clicked() {
                self.open_filter_popup(c);
                ui.close();
            }
            if row.is_some() {
                let value = self.sheet().display(self.sel.cursor);
                let short: String = value.chars().take(24).collect();
                if ui.button(format!("Filter by \"{short}\"")).clicked() {
                    let f = crate::model::filter::ColumnFilter {
                        conditions: vec![crate::model::filter::Condition {
                            op: crate::model::filter::Op::Eq,
                            value,
                        }],
                        allowed: None,
                    };
                    self.set_column_filter(c, Some(f));
                    ui.close();
                }
            }
            if self.view().filters.contains_key(&c) && ui.button("Clear filter").clicked() {
                self.set_column_filter(c, None);
                ui.close();
            }
            ui.separator();
            if ui.button("Autofit width").clicked() {
                self.autofit(ui, c);
                ui.close();
            }
        }
        if ui.button("Explain with AI").clicked() {
            self.ai.open = true;
            self.ai.task = crate::ai::Task::Explain;
            self.ai.focus_prompt = true;
            ui.close();
        }
    }

    /// Insert/delete items for the context menu: row headers get row items,
    /// column headers column items, and cells both.
    fn structural_menu(&mut self, ui: &mut egui::Ui, cols: bool, rows: bool) {
        let (_, nr) = self.selected_rows();
        let (_, nc) = self.selected_cols();
        let plural = |n: u32, one: &str, many: &str| {
            if n == 1 {
                one.to_string()
            } else {
                format!("{n} {many}")
            }
        };
        let (row_text, col_text) = (plural(nr, "Row", "Rows"), plural(nc, "Column", "Columns"));
        if rows {
            if ui.button(format!("Insert {row_text} Above")).clicked() {
                self.insert_rows(false);
                ui.close();
            }
            if ui.button(format!("Insert {row_text} Below")).clicked() {
                self.insert_rows(true);
                ui.close();
            }
        }
        if cols {
            if ui.button(format!("Insert {col_text} Left")).clicked() {
                self.insert_cols(false);
                ui.close();
            }
            if ui.button(format!("Insert {col_text} Right")).clicked() {
                self.insert_cols(true);
                ui.close();
            }
        }
        if rows && ui.button(format!("Delete {row_text}")).clicked() {
            self.delete_rows();
            ui.close();
        }
        if cols && ui.button(format!("Delete {col_text}")).clicked() {
            self.delete_cols();
            ui.close();
        }
        if rows || cols {
            ui.separator();
        }
    }
}

fn rgb(c: crate::model::Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// A text layout with the style's italic, underline and strikethrough.
fn text_job(text: String, font: &FontId, color: Color32, style: &crate::model::Style) -> LayoutJob {
    let line = |on: bool| {
        if on {
            Stroke::new(1.0, color)
        } else {
            Stroke::NONE
        }
    };
    LayoutJob::single_section(
        text,
        TextFormat {
            font_id: font.clone(),
            color,
            italics: style.italic,
            underline: line(style.underline),
            strikethrough: line(style.strike),
            ..Default::default()
        },
    )
}

fn paint_borders(painter: &egui::Painter, rect: Rect, b: crate::model::Borders, stroke: Stroke) {
    let r = rect.shrink(0.5);
    if b.top {
        painter.hline(r.x_range(), r.top(), stroke);
    }
    if b.bottom {
        painter.hline(r.x_range(), r.bottom(), stroke);
    }
    if b.left {
        painter.vline(r.left(), r.y_range(), stroke);
    }
    if b.right {
        painter.vline(r.right(), r.y_range(), stroke);
    }
}
