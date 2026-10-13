//! Sheet tabs: switching, adding, renaming and deleting sheets.

use egui::Key;

use super::app::App;
use super::selection::Selection;
use crate::model::filter::ViewState;

impl App {
    pub(crate) fn sheet_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mut switch = None;
            let mut delete = None;
            let mut start_rename = None;
            for i in 0..self.book.sheets.len() {
                if let Some((ri, buf)) = self.rename.as_mut()
                    && *ri == i
                {
                    let r = ui.add(egui::TextEdit::singleline(buf).desired_width(110.0));
                    r.request_focus();
                    let enter = ui.input(|inp| inp.key_pressed(Key::Enter));
                    let esc = ui.input(|inp| inp.key_pressed(Key::Escape));
                    if esc {
                        self.rename = None;
                    } else if r.lost_focus() || enter {
                        let name = buf.clone();
                        self.rename = None;
                        if let Err(e) = self.book.rename_sheet(i, &name) {
                            self.error(e);
                        }
                    }
                    continue;
                }
                let name = self.book.sheets[i].name.clone();
                let filtered = self.views[i].has_filters();
                let label = if filtered {
                    format!("{name} ⏷")
                } else {
                    name
                };
                let r = ui.add(egui::Button::selectable(i == self.active, label));
                if r.clicked() {
                    switch = Some(i);
                }
                if r.double_clicked() {
                    start_rename = Some(i);
                }
                r.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        start_rename = Some(i);
                    }
                    if ui
                        .add_enabled(self.book.sheets.len() > 1, egui::Button::new("Delete"))
                        .clicked()
                    {
                        delete = Some(i);
                    }
                });
            }
            if ui.button("+").on_hover_text("Add sheet").clicked() {
                self.commit_edit((0, 0));
                let name = format!("Sheet{}", self.book.sheets.len() + 1);
                let i = self.book.add_sheet(&name);
                self.book.dirty = true;
                self.views.push(ViewState::default());
                switch = Some(i);
            }
            if let Some(i) = switch {
                self.switch_sheet(i);
            }
            if let Some(i) = start_rename {
                self.rename = Some((i, self.book.sheets[i].name.clone()));
            }
            if let Some(i) = delete {
                self.commit_edit((0, 0));
                self.book.delete_sheet(i);
                self.views.remove(i);
                if self.active > i || self.active >= self.book.sheets.len() {
                    self.active -= 1;
                }
                self.sel = Selection::single((0, 0));
                self.data_changed();
            }
        });
    }
}
