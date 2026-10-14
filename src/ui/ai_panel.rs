//! The AI side panel: settings/key management, the four AI tasks, and the
//! spreadsheet context each task sends to the model.

use egui::{Id, RichText};

use super::app::App;
use crate::ai::{self, AiSettings, Task};
use crate::model::filter::{ColumnFilter, column_label, resolve_column};
use crate::model::refs;
use crate::model::{Sheet, Value};

const MAX_CONTEXT_CHARS: usize = 60_000;

pub struct AiPanel {
    pub open: bool,
    pub task: Task,
    pub prompt: String,
    pub focus_prompt: bool,
    pub show_settings: bool,
    settings: AiSettings,
    key_input: String,
    /// `None` until we've looked in the Keychain.
    key: Option<Option<String>>,
    busy: Option<Task>,
    result: Option<(Task, Result<String, String>)>,
    /// Where a generated formula will go.
    formula_target: Option<(u64, usize, (u32, u32))>,
    applied_filters: Vec<String>,
    last_sent: Option<(usize, String)>,
}

pub fn menu_label(task: Task) -> &'static str {
    match task {
        Task::Formula => "Generate Formula…",
        Task::Explain => "Explain Selection…",
        Task::Filter => "Filter with Plain English…",
        Task::Summarize => "Summarize Sheet…",
    }
}

impl AiPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            task: Task::Formula,
            prompt: String::new(),
            focus_prompt: false,
            show_settings: false,
            settings: AiSettings::load(),
            key_input: String::new(),
            key: None,
            busy: None,
            result: None,
            formula_target: None,
            applied_filters: Vec::new(),
            last_sent: None,
        }
    }

    fn key(&mut self) -> Option<String> {
        if self.key.is_none() {
            self.key = Some(ai::load_key());
        }
        self.key.clone().flatten()
    }

    /// Point the panel at a test server without touching the Keychain.
    #[cfg(test)]
    pub fn use_test_backend(&mut self, base_url: &str) {
        self.settings.base_url = base_url.to_string();
        self.key = Some(Some("test-key".into()));
    }

    #[cfg(test)]
    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    #[cfg(test)]
    pub fn result_text(&self) -> Option<Result<String, String>> {
        self.result.as_ref().map(|r| r.1.clone())
    }
}

impl App {
    pub fn poll_ai(&mut self) {
        while let Ok(resp) = self.ai_rx.try_recv() {
            self.ai.busy = None;
            let result = match (resp.task, resp.result) {
                (Task::Formula, Ok(text)) => ai::extract_formula(&text)
                    .ok_or_else(|| format!("The model didn't return a formula:\n{text}")),
                (Task::Filter, Ok(text)) => match self.apply_ai_filters(&text) {
                    Ok(desc) => Ok(desc),
                    Err(e) => Err(e),
                },
                (_, r) => r,
            };
            self.ai.result = Some((resp.task, result));
        }
    }

    fn apply_ai_filters(&mut self, reply: &str) -> Result<String, String> {
        let items = ai::parse_filters(reply)?;
        if items.is_empty() {
            return Err("The model didn't suggest any filters".into());
        }
        let sheet = self.sheet();
        let mut by_col: std::collections::BTreeMap<u32, ColumnFilter> = Default::default();
        for item in items {
            let col = resolve_column(sheet, &item.column)
                .ok_or_else(|| format!("Unknown column \"{}\" in the AI reply", item.column))?;
            by_col
                .entry(col)
                .or_default()
                .conditions
                .push(item.condition);
        }
        let v = &mut self.views[self.active];
        v.filters = by_col;
        self.data_changed();
        let s = &self.book.sheets[self.active];
        let v = &self.views[self.active];
        self.ai.applied_filters = v.describe_filters(s);
        let shown = v
            .rows
            .as_ref()
            .map(|r| r.len().saturating_sub(v.header_row as usize))
            .unwrap_or(0);
        Ok(format!("Showing {shown} matching rows."))
    }

    fn run_ai(&mut self, ctx: &egui::Context) {
        let Some(key) = self.ai.key() else {
            self.ai.show_settings = true;
            self.ai.result = Some((
                self.ai.task,
                Err("Add an API key in AI Settings first.".into()),
            ));
            return;
        };
        let task = self.ai.task;
        let context = self.ai_context(task);
        self.ai.last_sent = Some((context.len(), host_of(&self.ai.settings.base_url)));
        if task == Task::Formula {
            self.ai.formula_target = Some((self.doc_id(), self.active, self.sel.cursor));
        }
        self.ai.busy = Some(task);
        self.ai.result = None;
        self.ai.applied_filters.clear();
        ai::spawn(
            &self.rt,
            self.http.clone(),
            self.ai.settings.clone(),
            key,
            ai::Request {
                task,
                prompt: self.ai.prompt.clone(),
                context,
            },
            self.ai_tx.clone(),
            ctx.clone(),
        );
    }

    /* ------------------------------ context ------------------------------ */

    fn ai_context(&self, task: Task) -> String {
        let s = self.sheet();
        let header = self.view().header_row;
        let mut out = format!(
            "Sheet \"{}\" has {} rows and {} columns (used range A1:{}).\n",
            s.name,
            s.n_rows,
            s.n_cols,
            refs::cell_name(s.n_rows.max(1) - 1, s.n_cols.max(1) - 1)
        );
        if self.book.sheets.len() > 1 {
            let names: Vec<_> = self
                .book
                .sheets
                .iter()
                .map(|s| format!("\"{}\"", s.name))
                .collect();
            out.push_str(&format!(
                "Other sheets in the workbook: {}.\n",
                names.join(", ")
            ));
        }
        out.push_str(&column_summary(s, header));
        match task {
            Task::Formula => {
                out.push_str(&format!(
                    "\nThe formula will go in cell {}. Current selection: {}.\n",
                    refs::cell_name(self.sel.cursor.0, self.sel.cursor.1),
                    self.sel.name()
                ));
                out.push_str("\nFirst rows (row number, then cells):\n");
                out.push_str(&rows_tsv(s, 0, 15.min(s.n_rows), 0, s.n_cols, true));
            }
            Task::Explain => {
                let ((r0, c0), (r1, c1)) = self.sel.range();
                let r1 = r1.min(s.n_rows.max(1) - 1).min(r0 + 199);
                let c1 = c1.min(s.n_cols.max(1) - 1).min(c0 + 29);
                out.push_str(&format!("\nSelected range {} :\n", self.sel.name()));
                if header && r0 > 0 {
                    out.push_str(&rows_tsv(s, 0, 1, c0, c1 + 1, true));
                }
                out.push_str(&rows_tsv(s, r0, r1 + 1, c0, c1 + 1, true));
                if self.sel.is_single() {
                    let pos = self.sel.cursor;
                    if let Some(c) = s.cells.get(&pos).filter(|c| c.is_formula()) {
                        out.push_str(&format!(
                            "\nCell {} contains the formula {}\n",
                            self.sel.name(),
                            c.raw
                        ));
                    }
                }
            }
            Task::Filter => {
                out.push_str("\nSample values per column:\n");
                let v = self.view();
                for c in 0..s.n_cols.min(60) {
                    let vals = v.distinct_values(s, c, 12);
                    let vals: Vec<_> = vals.into_iter().filter(|x| !x.is_empty()).take(8).collect();
                    out.push_str(&format!(
                        "{}: {}\n",
                        column_label(s, c, header),
                        vals.join(" | ")
                    ));
                }
            }
            Task::Summarize => {
                out.push_str("\nRows (row number, then cells):\n");
                out.push_str(&rows_tsv(
                    s,
                    0,
                    s.n_rows.min(150),
                    0,
                    s.n_cols.min(40),
                    true,
                ));
                if s.n_rows > 150 {
                    out.push_str(&format!("… {} more rows not shown.\n", s.n_rows - 150));
                }
            }
        }
        if out.len() > MAX_CONTEXT_CHARS {
            let mut cut = MAX_CONTEXT_CHARS;
            while !out.is_char_boundary(cut) {
                cut -= 1;
            }
            out.truncate(cut);
            out.push_str("\n[truncated]\n");
        }
        out
    }

    /* -------------------------------- UI -------------------------------- */

    pub fn ai_panel_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("AI Assistant");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🗙").on_hover_text("Close panel").clicked() {
                    self.ai.open = false;
                }
                let gear = if self.ai.show_settings {
                    "Done"
                } else {
                    "⚙ Settings"
                };
                if ui.button(gear).clicked() {
                    self.ai.show_settings = !self.ai.show_settings;
                }
            });
        });
        ui.separator();

        let has_key = self.ai.key().is_some();
        if self.ai.show_settings || !has_key {
            self.ai_settings_ui(ui, has_key);
            ui.separator();
        }

        ui.horizontal(|ui| {
            for t in Task::ALL {
                if ui.selectable_label(self.ai.task == t, t.label()).clicked() && self.ai.task != t
                {
                    self.ai.task = t;
                    self.ai.focus_prompt = true;
                }
            }
        });
        ui.add_space(4.0);
        let scope = match self.ai.task {
            Task::Formula => format!(
                "Formula for cell {}",
                refs::cell_name(self.sel.cursor.0, self.sel.cursor.1)
            ),
            Task::Explain => format!("Explain {}", self.sel.name()),
            Task::Filter => format!("Filter rows of \"{}\"", self.sheet().name),
            Task::Summarize => format!("Summarize \"{}\"", self.sheet().name),
        };
        ui.label(RichText::new(scope).strong());

        let prompt_id = Id::new("ai_prompt");
        let resp = ui.add(
            egui::TextEdit::multiline(&mut self.ai.prompt)
                .id(prompt_id)
                .hint_text(self.ai.task.placeholder())
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if self.ai.focus_prompt {
            resp.request_focus();
            self.ai.focus_prompt = false;
        }
        let submit = resp.has_focus()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));

        let needs_prompt = matches!(self.ai.task, Task::Formula | Task::Filter);
        let can_run =
            self.ai.busy.is_none() && (!needs_prompt || !self.ai.prompt.trim().is_empty());
        ui.horizontal(|ui| {
            let label = match self.ai.task {
                Task::Formula => "Generate",
                Task::Explain => "Explain Selection",
                Task::Filter => "Apply Filter",
                Task::Summarize => "Summarize Sheet",
            };
            let run = ui
                .add_enabled(can_run, egui::Button::new(label))
                .on_hover_text("⌘↩");
            if (run.clicked() || (submit && can_run)) && can_run {
                self.run_ai(&ctx);
            }
            if self.ai.busy.is_some() {
                ui.spinner();
                ui.label("Thinking…");
            }
            if self.ai.task == Task::Filter
                && self.view().has_filters()
                && ui.button("Clear filters").clicked()
            {
                self.clear_filters();
                self.ai.applied_filters.clear();
            }
        });
        if let Some((bytes, host)) = &self.ai.last_sent {
            let size = if *bytes < 2048 {
                format!("{bytes} bytes")
            } else {
                format!("{} KB", bytes / 1024)
            };
            ui.label(
                RichText::new(format!("Last request sent {size} of sheet data to {host}."))
                    .weak()
                    .small(),
            );
        }
        ui.separator();

        self.ai_result_ui(ui, &ctx);
    }

    fn ai_result_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some((task, result)) = &self.ai.result else {
            ui.label(RichText::new(intro(self.ai.task)).weak());
            return;
        };
        let task = *task;
        match result.clone() {
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            Ok(text) => match task {
                Task::Formula => {
                    let mut f = text.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut f)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(1)
                            .desired_width(f32::INFINITY),
                    );
                    if f != text {
                        self.ai.result = Some((task, Ok(f.clone())));
                    }
                    ui.horizontal(|ui| {
                        if let Some((doc, sheet, pos)) = self.ai.formula_target
                            && doc == self.doc_id()
                            && sheet < self.book.sheets.len()
                        {
                            let label = format!("Insert into {}", refs::cell_name(pos.0, pos.1));
                            if ui.button(label).clicked() {
                                self.commit_edit((0, 0));
                                self.book.set_input(sheet, pos, &f);
                                if sheet != self.active {
                                    self.switch_sheet(sheet);
                                }
                                self.sel = super::Selection::single(pos);
                                self.scroll_to_cursor = true;
                                self.data_changed();
                                let shown = self.book.sheets[sheet].display(pos);
                                self.notify(format!("Inserted formula, result: {shown}"));
                            }
                        }
                        if ui.button("Copy").clicked() {
                            ctx.copy_text(f.clone());
                        }
                    });
                }
                Task::Filter => {
                    ui.label(text);
                    for d in &self.ai.applied_filters {
                        ui.label(format!("• {d}"));
                    }
                    ui.label(
                        RichText::new("Adjust from the ⏷ buttons in column headers.")
                            .weak()
                            .small(),
                    );
                }
                Task::Explain | Task::Summarize => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(text.clone()).selectable(true));
                        });
                    if ui.button("Copy").clicked() {
                        ctx.copy_text(text);
                    }
                }
            },
        }
    }

    fn ai_settings_ui(&mut self, ui: &mut egui::Ui, has_key: bool) {
        ui.label(RichText::new("Bring your own key").strong());
        ui.label(
            RichText::new(
                "Works with any OpenAI-compatible API (OpenAI, OpenRouter, Groq, Together, a local \
                 Ollama or LM Studio server, …). Your key is kept in the macOS Keychain and \
                 requests go straight from this app to that provider.",
            )
            .weak()
            .small(),
        );
        ui.add_space(4.0);
        egui::Grid::new("ai_settings")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("Base URL");
                ui.add(
                    egui::TextEdit::singleline(&mut self.ai.settings.base_url)
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();
                ui.label("Model");
                ui.add(
                    egui::TextEdit::singleline(&mut self.ai.settings.model)
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();
                ui.label("API key");
                ui.add(
                    egui::TextEdit::singleline(&mut self.ai.key_input)
                        .password(true)
                        .hint_text(if has_key {
                            "•••••• (saved)"
                        } else {
                            "sk-…"
                        })
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                let mut errors = Vec::new();
                if let Err(e) = self.ai.settings.save() {
                    errors.push(format!("Couldn't save settings: {e}"));
                }
                let key = self.ai.key_input.trim().to_string();
                if !key.is_empty() {
                    match ai::store_key(&key) {
                        Ok(()) => {
                            self.ai.key = Some(Some(key));
                            self.ai.key_input.clear();
                        }
                        Err(e) => errors.push(format!("Couldn't store key in Keychain: {e}")),
                    }
                }
                if errors.is_empty() {
                    self.notify("AI settings saved");
                    if self.ai.key().is_some() {
                        self.ai.show_settings = false;
                    }
                } else {
                    self.error(errors.join("; "));
                }
            }
            if has_key && ui.button("Remove key").clicked() {
                match ai::delete_key() {
                    Ok(()) => {
                        self.ai.key = Some(None);
                        self.notify("API key removed from Keychain");
                    }
                    Err(e) => self.error(format!("Couldn't remove key: {e}")),
                }
            }
            if has_key {
                ui.label(
                    RichText::new("✔ Key in Keychain").color(egui::Color32::from_rgb(60, 170, 90)),
                );
            }
        });
    }
}

fn intro(task: Task) -> &'static str {
    match task {
        Task::Formula => {
            "Describe what you want to calculate. The formula is written for the selected cell, using the sheet's column headers."
        }
        Task::Explain => {
            "Select a range and ask what it means, or leave the prompt empty for a general explanation."
        }
        Task::Filter => {
            "Describe the rows you want to see. The request becomes ordinary column filters that you can inspect and edit."
        }
        Task::Summarize => {
            "Get an overview of the current sheet: what it contains, key totals, trends and outliers."
        }
    }
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

/// "A = Name (text), B = Price (number) …" plus simple numeric stats.
fn column_summary(s: &Sheet, header: bool) -> String {
    let mut out = String::from("Columns:\n");
    let first = header as u32;
    for c in 0..s.n_cols.min(60) {
        let (mut nums, mut texts, mut sum) = (0usize, 0usize, 0.0f64);
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for r in first..s.n_rows.min(100_000) {
            match s.value((r, c)) {
                Value::Number(n) => {
                    nums += 1;
                    sum += n;
                    min = min.min(*n);
                    max = max.max(*n);
                }
                Value::Empty => {}
                _ => texts += 1,
            }
        }
        let kind = if nums > 0 && texts == 0 {
            format!(
                "numbers, {nums} values, sum {}, min {}, max {}",
                crate::model::fmt_number(sum),
                crate::model::fmt_number(min),
                crate::model::fmt_number(max)
            )
        } else if nums == 0 && texts == 0 {
            "empty".into()
        } else if nums == 0 {
            format!("text, {texts} values")
        } else {
            format!("mixed, {nums} numbers and {texts} text values")
        };
        out.push_str(&format!("- {}: {}\n", column_label(s, c, header), kind));
    }
    out
}

fn rows_tsv(s: &Sheet, r0: u32, r1: u32, c0: u32, c1: u32, row_numbers: bool) -> String {
    let mut out = String::new();
    out.push_str(if row_numbers { "#" } else { "" });
    for c in c0..c1 {
        out.push('\t');
        out.push_str(&refs::col_name(c));
    }
    out.push('\n');
    for r in r0..r1 {
        if row_numbers {
            out.push_str(&(r + 1).to_string());
        }
        for c in c0..c1 {
            out.push('\t');
            out.push_str(&s.display((r, c)).replace(['\t', '\n'], " "));
        }
        out.push('\n');
    }
    out
}
