//! The name box and formula bar above the grid.

use egui::{Id, Key, RichText};

use super::app::{App, BAR_EDITOR_ID, EditState};
use super::selection::Selection;
use super::util::set_cursor_end;
use crate::model::refs;

impl App {
    pub(crate) fn formula_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            // Name box: shows the selection, accepts a cell to jump to.
            // A selected merged cell is named by its top-left cell, like Excel.
            let shown = if self.sel_is_single_cell() {
                refs::cell_name(self.sel.cursor.0, self.sel.cursor.1)
            } else {
                self.sel.name()
            };
            let mut name = self.name_box.clone().unwrap_or(shown);
            let r = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(90.0)
                    .font(egui::TextStyle::Monospace)
                    .horizontal_align(egui::Align::Center),
            );
            if r.gained_focus() {
                self.commit_edit((0, 0));
            }
            if r.has_focus() {
                self.name_box = Some(name);
            } else if r.lost_focus() {
                if ctx.input(|i| i.key_pressed(Key::Enter)) {
                    self.goto(&name);
                }
                self.name_box = None;
            }
            ui.label(RichText::new("ƒx").italics().strong());

            let editor_id = Id::new(BAR_EDITOR_ID);
            let width = ui.available_width();
            if let Some(edit) = self.edit.as_mut() {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut edit.buf)
                        .id(editor_id)
                        .desired_width(width)
                        .font(egui::TextStyle::Monospace),
                );
                if edit.in_bar && edit.place_cursor && resp.has_focus() {
                    set_cursor_end(&ctx, editor_id, &edit.buf);
                    edit.place_cursor = false;
                }
                if resp.gained_focus() {
                    edit.in_bar = true;
                }
                if resp.changed() {
                    edit.pointing = None;
                }
                if edit.in_bar && edit.pointing.is_some() && ctx.memory(|m| m.focused().is_none()) {
                    resp.request_focus();
                }
            } else {
                let mut text = self.sheet().edit_text(self.sel.cursor);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id(editor_id)
                        .desired_width(width)
                        .font(egui::TextStyle::Monospace),
                );
                if resp.gained_focus() || resp.changed() {
                    self.edit = Some(EditState {
                        pos: self.sel.cursor,
                        buf: text,
                        in_bar: true,
                        place_cursor: false,
                        pointing: None,
                    });
                }
            }
        });
    }

    pub(crate) fn goto(&mut self, target: &str) {
        let t = target.trim();
        let mut parts = t.split(':');
        let a = parts.next().and_then(refs::parse_cell);
        let b = parts.next().and_then(refs::parse_cell);
        match (a, b) {
            (Some(a), Some(b)) => {
                self.sel = Selection {
                    anchor: a,
                    cursor: b,
                };
                self.scroll_to_cursor = true;
            }
            (Some(a), None) => self.set_cursor(a, false),
            _ => {
                if let Some(i) = self
                    .book
                    .sheets
                    .iter()
                    .position(|s| s.name.eq_ignore_ascii_case(t))
                {
                    self.switch_sheet(i);
                } else {
                    self.error(format!("\"{t}\" isn't a cell reference like B12 or A1:C5"));
                }
            }
        }
    }
}
