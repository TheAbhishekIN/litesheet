//! The application menu bar.

use egui::{Key, KeyboardShortcut, Modifiers, RichText};

use super::app::{App, Pending};
use super::util::sc_text;
use crate::ai;
use crate::model::filter::column_label;
use crate::model::{self, refs};

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
                if ui.button("Clear Formats").clicked() {
                    self.clear_formats();
                }
                if ui.button("Clear All").clicked() {
                    self.clear_all();
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
            ui.menu_button("Insert", |ui| {
                if ui.button("Chart…").clicked() {
                    self.open_chart_dialog(None);
                }
                ui.separator();
                if ui.button("Rows Above").clicked() {
                    self.insert_rows(false);
                }
                if ui.button("Columns Left").clicked() {
                    self.insert_cols(false);
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
            ui.menu_button("Format", |ui| self.format_menu(ui));
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
                let frozen = self.sheet().freeze;
                let cell = refs::cell_name(self.sel.cursor.0, self.sel.cursor.1);
                if ui.button(format!("Freeze Panes at {cell}")).clicked() {
                    self.freeze_at_cursor();
                }
                if ui.button("Freeze Top Row").clicked() {
                    self.set_freeze(model::Freeze { rows: 1, cols: 0 });
                }
                if ui.button("Freeze First Column").clicked() {
                    self.set_freeze(model::Freeze { rows: 0, cols: 1 });
                }
                if ui
                    .add_enabled(frozen.is_active(), egui::Button::new("Unfreeze Panes"))
                    .clicked()
                {
                    self.set_freeze(model::Freeze::default());
                }
                ui.separator();
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

    fn format_menu(&mut self, ui: &mut egui::Ui) {
        use model::{Currency, NumFmt};
        let current = self.sheet().fmt(self.sel.cursor);
        let same_kind =
            |a: NumFmt, b: NumFmt| std::mem::discriminant(&a) == std::mem::discriminant(&b);
        let mut item = |ui: &mut egui::Ui, label: &str, example: &str, fmt: NumFmt| {
            let button =
                egui::Button::selectable(same_kind(current, fmt), label).shortcut_text(example);
            if ui.add(button).clicked() {
                self.set_format(fmt);
                ui.close();
            }
        };
        item(ui, "General", "1234.5", NumFmt::General);
        item(
            ui,
            "Number",
            "1,234.50",
            NumFmt::Number {
                decimals: 2,
                thousands: true,
            },
        );
        ui.menu_button("Currency", |ui| {
            for symbol in Currency::ALL {
                let example = format!("{}1,234.50", symbol.symbol());
                item(
                    ui,
                    symbol.name(),
                    &example,
                    NumFmt::Currency {
                        decimals: 2,
                        symbol,
                    },
                );
            }
        });
        item(ui, "Percent", "12%", NumFmt::Percent { decimals: Some(0) });
        item(
            ui,
            "Scientific",
            "1.23E+04",
            NumFmt::Scientific { decimals: 2 },
        );
        ui.separator();
        item(ui, "Date", "2024-05-01", NumFmt::Date);
        item(ui, "Time", "14:30:00", NumFmt::Time);
        item(ui, "Date & Time", "2024-05-01 14:30", NumFmt::DateTime);
        ui.separator();
        item(ui, "Text", "00042", NumFmt::Text);
        ui.separator();
        let single = self.sel.is_single();
        if ui
            .add_enabled(!single, egui::Button::new("Merge & Center"))
            .clicked()
        {
            self.merge_selection(true);
        }
        if ui
            .add_enabled(!single, egui::Button::new("Merge Cells"))
            .clicked()
        {
            self.merge_selection(false);
        }
        if ui
            .add_enabled(
                self.selection_has_merge(),
                egui::Button::new("Unmerge Cells"),
            )
            .clicked()
        {
            self.unmerge_selection();
        }
        ui.separator();
        if ui.button("Increase Decimal").clicked() {
            self.adjust_decimals(1);
        }
        if ui.button("Decrease Decimal").clicked() {
            self.adjust_decimals(-1);
        }
    }
}
