//! The application menu bar.

use egui::{Key, KeyboardShortcut, Modifiers, RichText};

use super::app::{App, Pending};
use super::util::sc_text;
use crate::ai;
use crate::model;
use crate::model::filter::column_label;

impl App {
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("New").shortcut_text(sc_text(&ctx, Key::N, false)))
                    .clicked()
                {
                    self.request(Pending::New, &ctx);
                }
                if ui
                    .add(egui::Button::new("Open…").shortcut_text(sc_text(&ctx, Key::O, false)))
                    .clicked()
                {
                    self.request(Pending::Open, &ctx);
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Save").shortcut_text(sc_text(&ctx, Key::S, false)))
                    .clicked()
                {
                    self.save(false);
                }
                if ui
                    .add(egui::Button::new("Save As…").shortcut_text(sc_text(&ctx, Key::S, true)))
                    .clicked()
                {
                    self.save(true);
                }
                if ui.button("Export Sheet as CSV…").clicked() {
                    self.export_csv();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo = match self.book.undo_label() {
                    Some(l) => format!("Undo {l}"),
                    None => "Undo".into(),
                };
                if ui
                    .add_enabled(
                        self.book.can_undo(),
                        egui::Button::new(undo).shortcut_text(sc_text(&ctx, Key::Z, false)),
                    )
                    .clicked()
                {
                    self.undo();
                }
                let redo = match self.book.redo_label() {
                    Some(l) => format!("Redo {l}"),
                    None => "Redo".into(),
                };
                if ui
                    .add_enabled(
                        self.book.can_redo(),
                        egui::Button::new(redo).shortcut_text(sc_text(&ctx, Key::Z, true)),
                    )
                    .clicked()
                {
                    self.redo();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Cut").shortcut_text(sc_text(&ctx, Key::X, false)))
                    .clicked()
                {
                    self.copy(&ctx, true);
                }
                if ui
                    .add(egui::Button::new("Copy").shortcut_text(sc_text(&ctx, Key::C, false)))
                    .clicked()
                {
                    self.copy(&ctx, false);
                }
                ui.label(
                    RichText::new(format!(
                        "Paste: {}",
                        ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::V))
                    ))
                    .weak(),
                );
                if ui
                    .add(egui::Button::new("Clear").shortcut_text("Delete"))
                    .clicked()
                {
                    self.clear_selection();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Fill Down").shortcut_text(sc_text(&ctx, Key::D, false)))
                    .clicked()
                {
                    self.fill(true);
                }
                if ui
                    .add(egui::Button::new("Fill Right").shortcut_text(sc_text(
                        &ctx,
                        Key::R,
                        false,
                    )))
                    .clicked()
                {
                    self.fill(false);
                }
                ui.separator();
                let (_, nr) = self.selected_rows();
                let (_, nc) = self.selected_cols();
                let rows = if nr == 1 {
                    "Row".to_string()
                } else {
                    format!("{nr} Rows")
                };
                let cols = if nc == 1 {
                    "Column".to_string()
                } else {
                    format!("{nc} Columns")
                };
                if ui.button(format!("Insert {rows} Above")).clicked() {
                    self.insert_rows(false);
                }
                if ui.button(format!("Insert {cols} Left")).clicked() {
                    self.insert_cols(false);
                }
                if ui.button(format!("Delete {rows}")).clicked() {
                    self.delete_rows();
                }
                if ui.button(format!("Delete {cols}")).clicked() {
                    self.delete_cols();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Find…").shortcut_text(sc_text(&ctx, Key::F, false)))
                    .clicked()
                {
                    self.show_search = true;
                    self.focus_search = true;
                }
            });
            ui.menu_button("Data", |ui| {
                let col = self.sel.cursor.1;
                let label = column_label(self.sheet(), col, self.view().header_row);
                if ui.button(format!("Sort A to Z by {label}")).clicked() {
                    self.sort_by(col, true);
                }
                if ui.button(format!("Sort Z to A by {label}")).clicked() {
                    self.sort_by(col, false);
                }
                ui.separator();
                if ui
                    .add(
                        egui::Button::selectable(self.filter_buttons, "Filter Buttons")
                            .shortcut_text(sc_text(&ctx, Key::L, true)),
                    )
                    .clicked()
                {
                    self.filter_buttons = !self.filter_buttons;
                }
                if ui.button(format!("Filter {label}…")).clicked() {
                    self.open_filter_popup(col);
                }
                if ui
                    .add_enabled(
                        self.view().has_filters(),
                        egui::Button::new("Clear All Filters"),
                    )
                    .clicked()
                {
                    self.clear_filters();
                }
                ui.separator();
                let mut header = self.views[self.active].header_row;
                if ui.checkbox(&mut header, "First row is a header").changed() {
                    self.views[self.active].header_row = header;
                    self.data_changed();
                }
            });
            ui.menu_button("Format", |ui| {
                use model::NumFmt::*;
                for (fmt, label) in [
                    (General, "General"),
                    (Percent, "Percent"),
                    (Date, "Date"),
                    (DateTime, "Date & Time"),
                ] {
                    if ui.button(label).clicked() {
                        self.set_format(fmt);
                    }
                }
            });
            ui.menu_button("AI", |ui| {
                for task in ai::Task::ALL {
                    if ui.button(super::ai_panel::menu_label(task)).clicked() {
                        self.ai.open = true;
                        self.ai.task = task;
                        self.ai.focus_prompt = true;
                    }
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Show AI Panel").shortcut_text(sc_text(
                        &ctx,
                        Key::I,
                        true,
                    )))
                    .clicked()
                {
                    self.ai.open = !self.ai.open;
                }
                if ui.button("AI Settings…").clicked() {
                    self.ai.open = true;
                    self.ai.show_settings = true;
                }
            });
            ui.menu_button("View", |ui| {
                let mut dark = ui.visuals().dark_mode;
                if ui.checkbox(&mut dark, "Dark mode").changed() {
                    ctx.set_theme(if dark {
                        egui::Theme::Dark
                    } else {
                        egui::Theme::Light
                    });
                }
                ui.label(RichText::new("Zoom: ⌘+ / ⌘−").weak());
            });
        });
    }
}
