//! The find bar (⌘F).

use egui::{Id, Key};

use super::app::{App, SEARCH_ID};
use super::selection::Selection;

impl App {
    pub(crate) fn search_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label("🔍");
            let id = Id::new(SEARCH_ID);
            let mut q = self.views[self.active].search.clone();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut q)
                    .id(id)
                    .hint_text("Find in sheet")
                    .desired_width(240.0),
            );
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            if resp.changed() {
                self.views[self.active].search = q;
                self.views[self.active].current_match = 0;
                self.data_changed();
                self.jump_to_match(0);
            }
            let enter = resp.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter));
            if enter {
                let back = ctx.input(|i| i.modifiers.shift);
                self.next_match(if back { -1 } else { 1 });
                resp.request_focus();
            }
            if resp.has_focus() && ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.close_search();
                return;
            }
            let n = self.view().matches.len();
            let label = if self.view().search.trim().is_empty() {
                String::new()
            } else if n == 0 {
                "No matches".into()
            } else {
                format!("{} of {}", self.view().current_match + 1, n)
            };
            ui.label(label);
            if ui
                .add_enabled(n > 0, egui::Button::new("◀"))
                .on_hover_text("Previous (Shift+Return)")
                .clicked()
            {
                self.next_match(-1);
            }
            if ui
                .add_enabled(n > 0, egui::Button::new("▶"))
                .on_hover_text("Next (Return)")
                .clicked()
            {
                self.next_match(1);
            }
            let mut only = self.view().search_only_matching;
            if ui.checkbox(&mut only, "Only matching rows").changed() {
                self.views[self.active].search_only_matching = only;
                self.data_changed();
                self.ensure_cursor_visible();
            }
            if ui.button("🗙").on_hover_text("Close (Esc)").clicked() {
                self.close_search();
            }
        });
    }

    pub(crate) fn close_search(&mut self) {
        self.show_search = false;
        let v = &mut self.views[self.active];
        v.search.clear();
        v.search_only_matching = false;
        self.data_changed();
    }

    pub(crate) fn next_match(&mut self, delta: i64) {
        let n = self.view().matches.len() as i64;
        if n == 0 {
            return;
        }
        let i = (self.view().current_match as i64 + delta).rem_euclid(n) as usize;
        self.jump_to_match(i);
    }

    pub(crate) fn jump_to_match(&mut self, i: usize) {
        if let Some(&pos) = self.view().matches.get(i) {
            self.views[self.active].current_match = i;
            self.sel = Selection::single(pos);
            self.scroll_to_cursor = true;
        }
    }
}
