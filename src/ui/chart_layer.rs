//! Charts on the sheet: placing them over the grid, selecting, moving,
//! resizing and deleting them, and the Insert/Edit Chart dialog.

use egui::{Color32, CursorIcon, Id, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::app::App;
use super::chart;
use super::grid::{HEADER_H, Layout};
use crate::model::{Chart, ChartKind, Pos, refs};

const MIN_SIZE: egui::Vec2 = egui::Vec2::new(180.0, 120.0);

/// A chart being moved or resized with the mouse.
#[derive(Clone, Copy, Debug)]
pub struct ChartDrag {
    pub idx: usize,
    pub resize: bool,
    pub start: Pos2,
    pub rect: Rect,
}

/// State of the Insert/Edit Chart window.
#[derive(Clone, Debug)]
pub struct ChartDialog {
    /// The chart being edited, or `None` when inserting a new one.
    pub editing: Option<usize>,
    pub kind: ChartKind,
    pub title: String,
    /// Data range like `A1:C13` (optionally `Sheet!A1:C13`).
    pub range: String,
}

impl App {
    pub(super) fn chart_layer(&mut self, ui: &mut Ui, l: &Layout, visible: Rect) {
        let cells = Rect::from_min_max(
            pos2(l.left() + l.row_header_w, l.top() + HEADER_H),
            visible.max,
        );
        let pointer = ui.ctx().input(|i| i.pointer.interact_pos());
        let painter = ui.painter().with_clip_rect(cells);
        let accent = ui.visuals().selection.stroke.color;

        for idx in 0..self.sheet().charts.len() {
            let chart = self.sheet().charts[idx].clone();
            let Some(vi) = self.view_index(chart.anchor.0) else {
                continue;
            };
            if chart.anchor.1 as usize + 1 >= l.xs.len() {
                continue;
            }
            let mut rect = Rect::from_min_size(
                l.cell_rect(vi, chart.anchor.1).min,
                vec2(chart.size.0, chart.size.1),
            );
            if let (Some(d), Some(p)) = (self.chart_drag, pointer)
                && d.idx == idx
            {
                let delta = p - d.start;
                rect = if d.resize {
                    Rect::from_min_size(d.rect.min, (d.rect.size() + delta).max(MIN_SIZE))
                } else {
                    d.rect.translate(delta)
                };
            }
            if !rect.intersects(cells) {
                continue;
            }
            let resp = ui.interact(
                rect.intersect(cells),
                Id::new(("chart", self.active, idx)),
                Sense::click_and_drag(),
            );
            let handle =
                Rect::from_center_size(rect.right_bottom() - vec2(4.0, 4.0), vec2(14.0, 14.0));
            if let Some(h) = resp.hover_pos() {
                ui.ctx().set_cursor_icon(if handle.contains(h) {
                    CursorIcon::ResizeNwSe
                } else {
                    CursorIcon::Grab
                });
            }
            if resp.clicked() || resp.drag_started() {
                self.commit_edit((0, 0));
                self.selected_chart = Some(idx);
            }
            // Measure from where the button went down, not where egui decided
            // the drag began (a few pixels later), so the chart tracks the pointer.
            if resp.drag_started()
                && let Some(p) = ui
                    .ctx()
                    .input(|i| i.pointer.press_origin())
                    .or(resp.interact_pointer_pos())
            {
                self.chart_drag = Some(ChartDrag {
                    idx,
                    resize: handle.contains(p),
                    start: p,
                    rect,
                });
            }
            if resp.drag_stopped() && self.chart_drag.is_some_and(|d| d.idx == idx) {
                let d = self.chart_drag.take().unwrap();
                self.finish_chart_drag(l, idx, d.resize, rect);
            }
            if resp.double_clicked() {
                self.open_chart_dialog(Some(idx));
            }

            let data = chart.data(&self.book);
            chart::paint(
                &painter,
                rect,
                chart.kind,
                &chart.title,
                &data,
                ui.visuals(),
            );
            if self.selected_chart == Some(idx) {
                painter.rect_stroke(
                    rect,
                    6.0,
                    Stroke::new(2.0, accent),
                    egui::StrokeKind::Inside,
                );
                for corner in [
                    rect.left_top(),
                    rect.right_top(),
                    rect.left_bottom(),
                    rect.right_bottom(),
                ] {
                    let r = Rect::from_center_size(corner, vec2(8.0, 8.0));
                    painter.rect(
                        r,
                        1.0,
                        Color32::WHITE,
                        Stroke::new(1.5, accent),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
    }

    fn finish_chart_drag(&mut self, l: &Layout, idx: usize, resize: bool, rect: Rect) {
        let mut charts = self.sheet().charts.clone();
        let Some(chart) = charts.get_mut(idx) else {
            return;
        };
        if resize {
            chart.size = (rect.width().round(), rect.height().round());
            self.book.set_charts(self.active, charts, "Resize chart");
        } else {
            // Snap the top-left corner to the cell it was dropped on.
            let p = rect.min + vec2(1.0, 1.0);
            let row = self.row_at(l.row_at_y(p.y)).unwrap_or(0);
            chart.anchor = (row, l.col_at_x(p.x));
            self.book.set_charts(self.active, charts, "Move chart");
        }
    }

    pub(crate) fn delete_selected_chart(&mut self) {
        let Some(idx) = self.selected_chart.take() else {
            return;
        };
        let mut charts = self.sheet().charts.clone();
        if idx < charts.len() {
            charts.remove(idx);
            self.book.set_charts(self.active, charts, "Delete chart");
        }
    }

    /// Open the chart window for a new chart (from the selection) or an existing one.
    pub(crate) fn open_chart_dialog(&mut self, editing: Option<usize>) {
        self.commit_edit((0, 0));
        let dialog = match editing.and_then(|i| self.sheet().charts.get(i).map(|c| (i, c.clone())))
        {
            Some((i, c)) => ChartDialog {
                editing: Some(i),
                kind: c.kind,
                title: c.title.clone(),
                range: c
                    .data_block()
                    .map(|b| range_text(&b.sheet, b.start, b.end, &self.sheet().name))
                    .unwrap_or_default(),
            },
            None => {
                let (a, b) = if self.sel.is_single() {
                    self.current_region(self.sel.cursor)
                } else {
                    self.sel.range()
                };
                ChartDialog {
                    editing: None,
                    kind: ChartKind::Column,
                    title: String::new(),
                    range: range_text(&self.sheet().name, a, b, &self.sheet().name),
                }
            }
        };
        self.chart_dialog = Some(dialog);
    }

    /// The block of non-empty cells around `pos` (Excel's "current region").
    fn current_region(&self, pos: Pos) -> (Pos, Pos) {
        let s = self.sheet();
        let filled = |r: u32, c: u32| !s.display((r, c)).is_empty();
        let ((mut r0, mut c0), (mut r1, mut c1)) = (pos, pos);
        loop {
            let mut grew = false;
            if r0 > 0 && (c0..=c1).any(|c| filled(r0 - 1, c)) {
                r0 -= 1;
                grew = true;
            }
            if r1 + 1 < s.n_rows && (c0..=c1).any(|c| filled(r1 + 1, c)) {
                r1 += 1;
                grew = true;
            }
            if c0 > 0 && (r0..=r1).any(|r| filled(r, c0 - 1)) {
                c0 -= 1;
                grew = true;
            }
            if c1 + 1 < s.n_cols && (r0..=r1).any(|r| filled(r, c1 + 1)) {
                c1 += 1;
                grew = true;
            }
            if !grew {
                return ((r0, c0), (r1, c1));
            }
        }
    }

    /// Build a chart from the dialog's settings.
    fn chart_from_dialog(&self, d: &ChartDialog) -> Result<Chart, String> {
        let text = d.range.trim();
        let (sheet_name, range) = match text.rsplit_once('!') {
            Some((s, r)) => (s.trim_matches('\'').to_string(), r),
            None => (self.sheet().name.clone(), text),
        };
        let sheet = self
            .book
            .sheets
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&sheet_name))
            .ok_or_else(|| format!("There's no sheet named \"{sheet_name}\""))?;
        let (a, b) = range.split_once(':').unwrap_or((range, range));
        let (Some(a), Some(b)) = (refs::parse_cell(a), refs::parse_cell(b)) else {
            return Err("Enter a range like A1:C13".into());
        };
        let mut chart = Chart::from_block(sheet, a, b, d.kind)
            .ok_or("Select at least one column of numbers")?;
        if !d.title.trim().is_empty() {
            chart.title = d.title.trim().to_string();
        }
        if let Some(existing) = d.editing.and_then(|i| self.sheet().charts.get(i)) {
            chart.anchor = existing.anchor;
            chart.size = existing.size;
        }
        Ok(chart)
    }

    pub(crate) fn chart_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.chart_dialog.clone() else {
            return;
        };
        let mut open = true;
        let mut action = None;
        let built = self.chart_from_dialog(&d);
        let title = if d.editing.is_some() {
            "Edit Chart"
        } else {
            "Insert Chart"
        };
        egui::Window::new(title)
            .id(Id::new("chart_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for k in ChartKind::ALL {
                        ui.selectable_value(&mut d.kind, k, k.label());
                    }
                });
                ui.add_space(6.0);
                egui::Grid::new("chart_fields")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Title");
                        let hint = built.as_ref().map(|c| c.title.clone()).unwrap_or_default();
                        ui.add(
                            egui::TextEdit::singleline(&mut d.title)
                                .hint_text(if hint.is_empty() { "None".into() } else { hint })
                                .desired_width(300.0),
                        );
                        ui.end_row();
                        ui.label("Data");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.range)
                                .hint_text("A1:C13")
                                .desired_width(300.0),
                        );
                        ui.end_row();
                    });
                ui.add_space(6.0);
                let (rect, _) = ui.allocate_exact_size(vec2(400.0, 240.0), Sense::hover());
                match &built {
                    Ok(c) => {
                        let data = c.data(&self.book);
                        let title = if d.title.trim().is_empty() {
                            c.title.as_str()
                        } else {
                            d.title.trim()
                        };
                        chart::paint(ui.painter(), rect, d.kind, title, &data, ui.visuals());
                        ui.label(
                            RichText::new(format!(
                                "{} series · {} categories{}",
                                c.series.len(),
                                data.categories.len(),
                                if c.categories.is_some() {
                                    ""
                                } else {
                                    " (numbered)"
                                }
                            ))
                            .weak()
                            .small(),
                        );
                    }
                    Err(e) => {
                        ui.painter().rect_stroke(
                            rect,
                            6.0,
                            Stroke::new(1.0, ui.visuals().weak_text_color()),
                            egui::StrokeKind::Inside,
                        );
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let label = if d.editing.is_some() {
                        "Update Chart"
                    } else {
                        "Create Chart"
                    };
                    if ui
                        .add_enabled(built.is_ok(), egui::Button::new(label))
                        .clicked()
                    {
                        action = Some(true);
                    }
                    if d.editing.is_some() && ui.button("Delete Chart").clicked() {
                        action = Some(false);
                    }
                });
            });
        match action {
            Some(true) => {
                let chart = self.chart_from_dialog(&d).expect("checked above");
                let mut charts = self.sheet().charts.clone();
                let (label, idx) = match d.editing {
                    Some(i) if i < charts.len() => {
                        charts[i] = chart;
                        ("Edit chart", i)
                    }
                    _ => {
                        charts.push(chart);
                        ("Insert chart", charts.len() - 1)
                    }
                };
                self.book.set_charts(self.active, charts, label);
                self.selected_chart = Some(idx);
                self.chart_dialog = None;
            }
            Some(false) => {
                self.selected_chart = d.editing;
                self.delete_selected_chart();
                self.chart_dialog = None;
            }
            None if !open => self.chart_dialog = None,
            None => self.chart_dialog = Some(d),
        }
    }
}

/// `A1:C13`, or `Other!A1:C13` when the range is on another sheet.
fn range_text(sheet: &str, a: Pos, b: Pos, current: &str) -> String {
    let range = format!(
        "{}:{}",
        refs::cell_name(a.0, a.1),
        refs::cell_name(b.0, b.1)
    );
    if sheet.eq_ignore_ascii_case(current) {
        range
    } else {
        format!("{}!{range}", refs::quote_sheet(sheet))
    }
}
