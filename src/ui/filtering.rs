//! Column filters: the filter window and applying/clearing filters.

use egui::{Context, Id, RichText};

use super::app::{App, FilterPopup};
use super::selection::Selection;
use super::util::rand_serial;
use crate::model::filter::{ColumnFilter, Condition, Op, column_label};

impl App {
    pub(crate) fn open_filter_popup(&mut self, col: u32) {
        let view = self.view();
        let existing = view.filters.get(&col).cloned().unwrap_or_default();
        let distinct = view.distinct_values(self.sheet(), col, 2000);
        let truncated = distinct.len() >= 2000;
        let values = distinct
            .into_iter()
            .map(|v| {
                let on = existing.allowed.as_ref().is_none_or(|a| a.contains(&v));
                (v, on)
            })
            .collect();
        let mut conds = [(Op::Contains, String::new()), (Op::Contains, String::new())];
        for (i, c) in existing.conditions.iter().take(2).enumerate() {
            conds[i] = (c.op, c.value.clone());
        }
        self.filter_popup = Some(FilterPopup {
            col,
            use_second: existing.conditions.len() > 1,
            conds,
            values,
            value_search: String::new(),
            truncated,
            anchor: None,
            serial: self.data_version ^ (col as u64) << 32 ^ rand_serial(),
        });
        self.filter_buttons = true;
    }

    pub(crate) fn set_column_filter(&mut self, col: u32, filter: Option<ColumnFilter>) {
        match filter {
            Some(f) if f.is_active() => {
                self.views[self.active].filters.insert(col, f);
            }
            _ => {
                self.views[self.active].filters.remove(&col);
            }
        }
        self.data_changed();
        self.ensure_cursor_visible();
    }

    pub(crate) fn clear_filters(&mut self) {
        self.views[self.active].filters.clear();
        self.data_changed();
    }

    pub(crate) fn ensure_cursor_visible(&mut self) {
        if self.view_index(self.sel.cursor.0).is_none() {
            let r = self
                .row_at(self.view().header_row as u32)
                .or_else(|| self.row_at(0))
                .unwrap_or(0);
            self.sel = Selection::single((r, self.sel.cursor.1));
        }
    }

    pub(crate) fn filter_window(&mut self, ctx: &Context) {
        let Some(popup) = self.filter_popup.as_mut() else {
            return;
        };
        let col = popup.col;
        let title = format!(
            "Filter {}",
            column_label(
                &self.book.sheets[self.active],
                col,
                self.views[self.active].header_row
            )
        );
        let mut open = true;
        let mut action: Option<Option<ColumnFilter>> = None;
        let mut window = egui::Window::new(title).id(Id::new(("filter_window", popup.serial)));
        if let Some(p) = popup.anchor {
            window = window.default_pos(p);
        }
        window
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Condition").strong());
                for i in 0..2 {
                    if i == 1 && !popup.use_second {
                        if ui.small_button("+ AND another condition").clicked() {
                            popup.use_second = true;
                        }
                        break;
                    }
                    ui.horizontal(|ui| {
                        let (op, val) = &mut popup.conds[i];
                        egui::ComboBox::from_id_salt(("filter_op", i))
                            .selected_text(op.label())
                            .width(130.0)
                            .show_ui(ui, |ui| {
                                for o in Op::ALL {
                                    ui.selectable_value(op, o, o.label());
                                }
                            });
                        if op.needs_value() {
                            ui.add(egui::TextEdit::singleline(val).desired_width(120.0));
                        }
                    });
                }
                ui.separator();
                ui.label(RichText::new("Values").strong());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut popup.value_search)
                            .hint_text("Search values")
                            .desired_width(150.0),
                    );
                    let q = popup.value_search.to_lowercase();
                    if ui.small_button("All").clicked() {
                        popup
                            .values
                            .iter_mut()
                            .filter(|v| v.0.to_lowercase().contains(&q))
                            .for_each(|v| v.1 = true);
                    }
                    if ui.small_button("None").clicked() {
                        popup
                            .values
                            .iter_mut()
                            .filter(|v| v.0.to_lowercase().contains(&q))
                            .for_each(|v| v.1 = false);
                    }
                });
                let q = popup.value_search.to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for (v, on) in popup.values.iter_mut() {
                            if !q.is_empty() && !v.to_lowercase().contains(&q) {
                                continue;
                            }
                            let label = if v.is_empty() { "(Blanks)" } else { v.as_str() };
                            ui.checkbox(on, label);
                        }
                    });
                if popup.truncated {
                    ui.label(
                        RichText::new("Showing the first 2000 distinct values")
                            .weak()
                            .small(),
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        let mut conditions = Vec::new();
                        for i in 0..if popup.use_second { 2 } else { 1 } {
                            let (op, val) = &popup.conds[i];
                            if !op.needs_value() || !val.trim().is_empty() {
                                conditions.push(Condition {
                                    op: *op,
                                    value: val.clone(),
                                });
                            }
                        }
                        let all_on = popup.values.iter().all(|v| v.1);
                        let allowed = (!all_on
                            || popup.truncated && !popup.values.iter().all(|v| v.1))
                        .then(|| {
                            popup
                                .values
                                .iter()
                                .filter(|v| v.1)
                                .map(|v| v.0.clone())
                                .collect()
                        });
                        action = Some(Some(ColumnFilter {
                            conditions,
                            allowed,
                        }));
                    }
                    if ui.button("Clear").clicked() {
                        action = Some(None);
                    }
                });
            });
        if let Some(f) = action {
            self.set_column_filter(col, f);
            self.filter_popup = None;
        } else if !open {
            self.filter_popup = None;
        }
    }
}
