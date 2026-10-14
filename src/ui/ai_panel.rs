//! The AI side panel: a chat about the open spreadsheet. Each request sends
//! a fresh snapshot of the active sheet; replies can carry a formula to
//! insert or filters to apply, which show up as buttons.

use egui::text::LayoutJob;
use egui::{Align, FontId, Id, Key, Layout, Margin, Modifiers, RichText, TextFormat};
use tokio::task::JoinHandle;

use super::app::App;
use super::util::file_name;
use crate::ai::{self, AiSettings, FilterItem, Message, Part, Role, Task};
use crate::model::filter::{ColumnFilter, column_label, resolve_column};
use crate::model::refs;
use crate::model::{Pos, Sheet, Value};

const MAX_CONTEXT_CHARS: usize = 60_000;
const PROMPT_ID: &str = "ai_prompt";

/// A workbook (document id), sheet and cell.
type Target = (u64, usize, Pos);

/// One message in the chat.
pub struct ChatMsg {
    pub role: Role,
    pub text: String,
    /// The reply split into text, formulas and filters (assistant messages).
    pub parts: Vec<Part>,
    /// A failed request; it isn't sent back to the model.
    pub error: bool,
    /// Where a formula from this reply goes: the workbook, sheet and cell
    /// that were selected when the question was asked.
    target: Option<Target>,
    /// Feedback after acting on the reply, e.g. "Showing 3 matching rows".
    note: Option<String>,
}

impl ChatMsg {
    fn user(text: String) -> Self {
        Self {
            role: Role::User,
            text,
            parts: Vec::new(),
            error: false,
            target: None,
            note: None,
        }
    }

    fn assistant(text: String, error: bool, target: Option<Target>) -> Self {
        Self {
            role: Role::Assistant,
            parts: if error {
                Vec::new()
            } else {
                ai::parse_reply(&text)
            },
            text,
            error,
            target,
            note: None,
        }
    }
}

pub struct AiPanel {
    pub open: bool,
    pub prompt: String,
    pub focus_prompt: bool,
    pub show_settings: bool,
    pub chat: Vec<ChatMsg>,
    settings: AiSettings,
    key_input: String,
    /// `None` until we've looked in the Keychain.
    key: Option<Option<String>>,
    /// The request in flight, and where its formula would go.
    busy: Option<(u64, JoinHandle<()>, Option<Target>)>,
    next_id: u64,
    last_sent: Option<(usize, String)>,
}

pub fn menu_label(task: Task) -> &'static str {
    match task {
        Task::Formula => "Write a Formula…",
        Task::Explain => "Explain Selection",
        Task::Filter => "Filter Rows…",
        Task::Summarize => "Summarize Sheet",
    }
}

fn suggestion_label(task: Task) -> &'static str {
    match task {
        Task::Formula => "🖩  Write a formula…",
        Task::Explain => "🔍  Explain the selected cells",
        Task::Filter => "⏷  Show only some rows…",
        Task::Summarize => "📊  Summarize this sheet",
    }
}

impl AiPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            prompt: String::new(),
            focus_prompt: false,
            show_settings: false,
            chat: Vec::new(),
            settings: AiSettings::load(),
            key_input: String::new(),
            key: None,
            busy: None,
            next_id: 0,
            last_sent: None,
        }
    }

    fn key(&mut self) -> Option<String> {
        if self.key.is_none() {
            self.key = Some(ai::load_key());
        }
        self.key.clone().flatten()
    }

    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    fn stop(&mut self) {
        if let Some((_, handle, _)) = self.busy.take() {
            handle.abort();
        }
    }

    /// Point the panel at a test server without touching the Keychain.
    #[cfg(test)]
    pub fn use_test_backend(&mut self, base_url: &str) {
        self.settings.base_url = base_url.to_string();
        self.key = Some(Some("test-key".into()));
    }

    /// The last assistant message, for tests.
    #[cfg(test)]
    pub fn last_reply(&self) -> Option<&ChatMsg> {
        self.chat.iter().rev().find(|m| m.role == Role::Assistant)
    }
}

impl App {
    pub fn poll_ai(&mut self) {
        while let Ok(resp) = self.ai_rx.try_recv() {
            let Some((id, _, target)) = &self.ai.busy else {
                continue;
            };
            if *id != resp.id {
                continue; // stopped or superseded
            }
            let target = *target;
            self.ai.busy = None;
            let msg = match resp.result {
                Ok(text) => ChatMsg::assistant(text, false, target),
                Err(e) => ChatMsg::assistant(e, true, None),
            };
            self.ai.chat.push(msg);
        }
    }

    /// Add the user's message to the chat and ask the model.
    pub fn ai_send(&mut self, text: String, ctx: &egui::Context) {
        let text = text.trim().to_string();
        if text.is_empty() || self.ai.is_busy() {
            return;
        }
        self.ai.open = true;
        self.ai.chat.push(ChatMsg::user(text));
        self.ai_request(ctx);
    }

    /// Send the conversation, which ends with a user message.
    fn ai_request(&mut self, ctx: &egui::Context) {
        let Some(key) = self.ai.key() else {
            self.ai.show_settings = true;
            self.ai.chat.push(ChatMsg::assistant(
                "Add an API key in AI Settings first.".into(),
                true,
                None,
            ));
            return;
        };
        let history = self
            .ai
            .chat
            .iter()
            .filter(|m| !m.error)
            .map(|m| Message {
                role: m.role,
                text: m.text.clone(),
            })
            .collect();
        let context = self.ai_context();
        self.ai.last_sent = Some((context.len(), host_of(&self.ai.settings.base_url)));
        self.ai.next_id += 1;
        let id = self.ai.next_id;
        let handle = ai::spawn(
            &self.rt,
            self.http.clone(),
            self.ai.settings.clone(),
            key,
            ai::Request {
                history,
                context,
                id,
            },
            self.ai_tx.clone(),
            ctx.clone(),
        );
        let target = Some((self.doc_id(), self.active, self.sel.cursor));
        self.ai.busy = Some((id, handle, target));
    }

    /// The shortcuts in the AI menu and the empty chat: some ask right
    /// away, others start a message for the user to finish.
    pub fn ai_quick(&mut self, task: Task, ctx: &egui::Context) {
        self.ai.open = true;
        self.ai.show_settings = false;
        match task {
            Task::Summarize => self.ai_send("Summarize this sheet.".into(), ctx),
            Task::Explain => {
                let msg = format!("Explain the selected cells ({}).", self.sel.name());
                self.ai_send(msg, ctx);
            }
            Task::Formula => {
                let cell = refs::cell_name(self.sel.cursor.0, self.sel.cursor.1);
                self.ai.prompt = format!("Write a formula for {cell} that ");
                self.ai.focus_prompt = true;
            }
            Task::Filter => {
                self.ai.prompt = "Show only rows where ".into();
                self.ai.focus_prompt = true;
            }
        }
    }

    fn apply_ai_filters(&mut self, items: &[FilterItem]) -> Result<String, String> {
        let sheet = self.sheet();
        let mut by_col: std::collections::BTreeMap<u32, ColumnFilter> = Default::default();
        for item in items {
            let col = resolve_column(sheet, &item.column)
                .ok_or_else(|| format!("Unknown column \"{}\"", item.column))?;
            by_col
                .entry(col)
                .or_default()
                .conditions
                .push(item.condition.clone());
        }
        let v = &mut self.views[self.active];
        v.filters = by_col;
        self.data_changed();
        let v = &self.views[self.active];
        let shown = v
            .rows
            .as_ref()
            .map(|r| r.len().saturating_sub(v.header_row as usize))
            .unwrap_or(0);
        Ok(format!(
            "Showing {shown} matching rows. Adjust them from the ⏷ buttons in the column headers."
        ))
    }

    fn insert_ai_formula(&mut self, formula: &str, target: Option<Target>) {
        let (sheet, pos) = match target {
            Some((doc, sheet, pos)) if doc == self.doc_id() && sheet < self.book.sheets.len() => {
                (sheet, pos)
            }
            _ => (self.active, self.sel.cursor),
        };
        self.commit_edit((0, 0));
        self.book.set_input(sheet, pos, formula);
        if sheet != self.active {
            self.switch_sheet(sheet);
        }
        self.sel = super::Selection::single(pos);
        self.scroll_to_cursor = true;
        self.data_changed();
        let shown = self.book.sheets[sheet].display(pos);
        self.notify(format!(
            "Inserted formula into {}, result: {shown}",
            refs::cell_name(pos.0, pos.1)
        ));
    }

    /* ------------------------------ context ------------------------------ */

    /// What the model sees: the sheet's shape and columns, the selection,
    /// and as many rows as fit.
    fn ai_context(&self) -> String {
        let s = self.sheet();
        let v = self.view();
        let header = v.header_row;
        let file = self
            .book
            .path
            .as_deref()
            .map(file_name)
            .unwrap_or_else(|| "Untitled".into());
        let mut out = format!(
            "Workbook \"{file}\". Active sheet \"{}\" has {} rows and {} columns (used range A1:{}).\n",
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
            out.push_str(&format!("Sheets in the workbook: {}.\n", names.join(", ")));
        }
        if v.has_filters() {
            let shown = v.rows.as_ref().map_or(0, |r| r.len());
            out.push_str(&format!(
                "Filters are on ({}); {shown} rows are visible.\n",
                v.describe_filters(s).join("; ")
            ));
        }
        out.push_str(&column_summary(s, header));

        let cursor = self.sel.cursor;
        out.push_str(&format!(
            "\nThe user has selected {} (active cell {}",
            self.sel.name(),
            refs::cell_name(cursor.0, cursor.1)
        ));
        match s.cells.get(&cursor) {
            Some(c) if c.is_formula() => out.push_str(&format!(
                ", which contains {} = {}",
                c.raw,
                s.display(cursor)
            )),
            Some(_) => out.push_str(&format!(", value \"{}\"", s.display(cursor))),
            None => out.push_str(", empty"),
        }
        out.push_str(").\n");
        if !self.sel.is_single() {
            let ((r0, c0), (r1, c1)) = self.sel.range();
            let r1 = r1.min(s.n_rows.max(1) - 1).min(r0 + 199);
            let c1 = c1.min(s.n_cols.max(1) - 1).min(c0 + 29);
            out.push_str("Selected cells:\n");
            if header && r0 > 0 {
                out.push_str(&rows_tsv(s, 0, 1, c0, c1 + 1, true));
            }
            out.push_str(&rows_tsv(s, r0, r1 + 1, c0, c1 + 1, true));
        }

        out.push_str("\nSample values per column:\n");
        for c in 0..s.n_cols.min(60) {
            let vals = v.distinct_values(s, c, 12);
            let vals: Vec<_> = vals.into_iter().filter(|x| !x.is_empty()).take(8).collect();
            out.push_str(&format!(
                "{}: {}\n",
                column_label(s, c, header),
                vals.join(" | ")
            ));
        }

        out.push_str("\nRows (row number, then cells):\n");
        let cols = s.n_cols.min(40);
        out.push_str(&rows_tsv(s, 0, 0, 0, cols, true));
        let mut shown = 0;
        while shown < s.n_rows && out.len() < MAX_CONTEXT_CHARS {
            out.push_str(
                rows_tsv(s, shown, shown + 1, 0, cols, true)
                    .split_once('\n')
                    .map_or("", |x| x.1),
            );
            shown += 1;
        }
        if shown < s.n_rows {
            out.push_str(&format!(
                "… {} more rows not shown (column statistics above cover all rows).\n",
                s.n_rows - shown
            ));
        }
        out
    }

    /* -------------------------------- UI -------------------------------- */

    pub fn ai_panel_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("AI Assistant");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("🗙").on_hover_text("Close panel").clicked() {
                    self.ai.open = false;
                }
                if ui
                    .selectable_label(self.ai.show_settings, "⚙")
                    .on_hover_text("AI settings")
                    .clicked()
                {
                    self.ai.show_settings = !self.ai.show_settings;
                }
                if ui
                    .add_enabled(!self.ai.chat.is_empty(), egui::Button::new("New chat"))
                    .on_hover_text("Start a new conversation")
                    .clicked()
                {
                    self.ai.stop();
                    self.ai.chat.clear();
                    self.ai.focus_prompt = true;
                }
            });
        });
        ui.separator();

        let has_key = self.ai.key().is_some();
        if self.ai.show_settings || !has_key {
            egui::ScrollArea::vertical().show(ui, |ui| self.ai_settings_ui(ui, has_key));
            return;
        }

        egui::Panel::bottom("ai_composer")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| self.ai_composer(ui, &ctx));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if self.ai.chat.is_empty() && !self.ai.is_busy() {
                            self.ai_empty_state(ui, &ctx);
                        } else {
                            self.ai_messages(ui, &ctx);
                        }
                    });
            });
    }

    fn ai_empty_state(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("💬").size(28.0));
            ui.add_space(6.0);
            ui.label(
                RichText::new("Ask anything about your data")
                    .strong()
                    .size(15.0),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "I can see this sheet's columns, a sample of its rows and your selection.",
                )
                .weak(),
            );
        });
        ui.add_space(16.0);
        for task in Task::ALL {
            let button = egui::Button::new(suggestion_label(task))
                .min_size(egui::vec2(ui.available_width(), 30.0))
                .corner_radius(8.0);
            if ui.add(button).clicked() {
                self.ai_quick(task, ctx);
            }
            ui.add_space(2.0);
        }
    }

    fn ai_messages(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(6.0);
        let max_w = (ui.available_width() * 0.88).max(120.0);
        let mut retry = false;
        let mut insert = None;
        let mut apply = None;
        let last = self.ai.chat.len().saturating_sub(1);
        for (i, msg) in self.ai.chat.iter().enumerate() {
            match msg.role {
                Role::User => {
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        egui::Frame::new()
                            .fill(ui.visuals().selection.bg_fill)
                            .corner_radius(12.0)
                            .inner_margin(Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.set_max_width(max_w);
                                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                    let color = ui.visuals().selection.stroke.color;
                                    ui.add(
                                        egui::Label::new(RichText::new(&msg.text).color(color))
                                            .wrap()
                                            .selectable(true),
                                    );
                                });
                            });
                    });
                }
                Role::Assistant => {
                    let frame = egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                        .corner_radius(12.0)
                        .inner_margin(Margin::symmetric(10, 8));
                    frame.show(ui, |ui| {
                        ui.set_max_width(max_w);
                        if msg.error {
                            ui.colored_label(ui.visuals().error_fg_color, &msg.text);
                            if i == last && ui.button("Retry").clicked() {
                                retry = true;
                            }
                            return;
                        }
                        for (p, part) in msg.parts.iter().enumerate() {
                            match part {
                                Part::Text(t) => rich_text(ui, t),
                                Part::Code(code) => code_block(ui, code),
                                Part::Formula(f) => {
                                    code_block(ui, f);
                                    ui.horizontal(|ui| {
                                        let cell = match msg.target {
                                            Some((doc, _, pos)) if doc == self.doc_id() => pos,
                                            _ => self.sel.cursor,
                                        };
                                        let label = format!(
                                            "Insert into {}",
                                            refs::cell_name(cell.0, cell.1)
                                        );
                                        if ui.button(label).clicked() {
                                            insert = Some((f.clone(), msg.target));
                                        }
                                        if ui.button("Copy").clicked() {
                                            ctx.copy_text(f.clone());
                                        }
                                    });
                                }
                                Part::Filter(items) => {
                                    let s = self.sheet();
                                    let header = self.view().header_row;
                                    for item in items {
                                        let col = resolve_column(s, &item.column)
                                            .map(|c| column_label(s, c, header))
                                            .unwrap_or_else(|| item.column.clone());
                                        ui.label(format!("• {col} {}", item.condition.describe()));
                                    }
                                    if ui.button("Apply filter").clicked() {
                                        apply = Some((i, p));
                                    }
                                }
                            }
                            ui.add_space(4.0);
                        }
                        if let Some(note) = &msg.note {
                            ui.label(RichText::new(note).weak().small());
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::Button::new(RichText::new("Copy").small()).frame(false))
                                .on_hover_text("Copy the whole reply")
                                .clicked()
                            {
                                ctx.copy_text(msg.text.clone());
                            }
                        });
                    });
                }
            }
            ui.add_space(8.0);
        }
        if self.ai.is_busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Thinking…").weak());
            });
        }

        if retry {
            self.ai.chat.pop();
            self.ai_request(ctx);
        }
        if let Some((f, target)) = insert {
            self.insert_ai_formula(&f, target);
        }
        if let Some((i, p)) = apply
            && let Some(Part::Filter(items)) = self.ai.chat[i].parts.get(p).cloned()
        {
            let note = match self.apply_ai_filters(&items) {
                Ok(n) => n,
                Err(e) => e,
            };
            self.ai.chat[i].note = Some(note);
        }
    }

    fn ai_composer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(6.0);
        let file = self
            .book
            .path
            .as_deref()
            .map(file_name)
            .unwrap_or_else(|| "Untitled".into());
        ui.label(
            RichText::new(format!(
                "📄 {file} › {} · {} selected",
                self.sheet().name,
                self.sel.name()
            ))
            .weak()
            .small(),
        );
        let prompt_id = Id::new(PROMPT_ID);
        let focused = ctx.memory(|m| m.has_focus(prompt_id));
        // Enter sends; Shift+Enter starts a new line.
        let enter = focused
            && ctx.input_mut(|i| !i.modifiers.shift && i.consume_key(Modifiers::NONE, Key::Enter));
        let resp = ui.add(
            egui::TextEdit::multiline(&mut self.ai.prompt)
                .id(prompt_id)
                .hint_text("Ask about your data…")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        if self.ai.focus_prompt {
            resp.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ctx, prompt_id) {
                let end = egui::text::CCursor::new(self.ai.prompt.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(ctx, prompt_id);
            }
            self.ai.focus_prompt = false;
        }
        ui.horizontal(|ui| {
            if let Some((bytes, host)) = &self.ai.last_sent {
                let size = if *bytes < 2048 {
                    format!("{bytes} bytes")
                } else {
                    format!("{} KB", bytes / 1024)
                };
                ui.label(
                    RichText::new(format!("Last sent {size} to {host}"))
                        .weak()
                        .small(),
                );
            } else {
                ui.label(
                    RichText::new(format!(
                        "Shares a sheet summary with {}",
                        host_of(&self.ai.settings.base_url)
                    ))
                    .weak()
                    .small(),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.ai.is_busy() {
                    if ui.button("■ Stop").clicked() {
                        self.ai.stop();
                    }
                } else {
                    let can_send = !self.ai.prompt.trim().is_empty();
                    let send = ui
                        .add_enabled(can_send, egui::Button::new("Send"))
                        .on_hover_text("Enter to send, Shift+Enter for a new line");
                    if (send.clicked() || enter) && can_send {
                        let text = std::mem::take(&mut self.ai.prompt);
                        self.ai_send(text, ctx);
                        resp.request_focus();
                    }
                }
            });
        });
        ui.add_space(6.0);
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

/// Text with `**bold**`, `` `code` `` and "- " bullets.
fn rich_text(ui: &mut egui::Ui, text: &str) {
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            ui.add_space(4.0);
            continue;
        }
        let trimmed = line.trim_start();
        let bullet = ["- ", "* ", "• "]
            .iter()
            .find_map(|b| trimmed.strip_prefix(b));
        let heading = trimmed.trim_start_matches('#');
        if let Some(rest) = bullet {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label("•");
                ui.add(
                    egui::Label::new(inline(ui, rest, false))
                        .wrap()
                        .selectable(true),
                );
            });
        } else if heading.len() < trimmed.len() {
            ui.add(
                egui::Label::new(inline(ui, heading.trim(), true))
                    .wrap()
                    .selectable(true),
            );
        } else {
            ui.add(
                egui::Label::new(inline(ui, trimmed, false))
                    .wrap()
                    .selectable(true),
            );
        }
    }
}

fn inline(ui: &egui::Ui, text: &str, bold_all: bool) -> LayoutJob {
    let style = ui.style();
    let body = FontId::proportional(style.text_styles[&egui::TextStyle::Body].size);
    let mono = FontId::monospace(body.size - 1.0);
    let normal = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    let mut job = LayoutJob::default();
    let mut bold = false;
    let mut rest = text;
    while !rest.is_empty() {
        let next_bold = rest.find("**");
        let next_code = rest.find('`');
        let cut = match (next_bold, next_code) {
            (Some(b), Some(c)) => b.min(c),
            (Some(b), None) => b,
            (None, Some(c)) => c,
            (None, None) => rest.len(),
        };
        let fmt = TextFormat::simple(body.clone(), if bold || bold_all { strong } else { normal });
        job.append(&rest[..cut], 0.0, fmt);
        rest = &rest[cut..];
        if let Some(r) = rest.strip_prefix("**") {
            bold = !bold;
            rest = r;
        } else if let Some(r) = rest.strip_prefix('`') {
            let end = r.find('`').unwrap_or(r.len());
            let mut fmt = TextFormat::simple(mono.clone(), strong);
            fmt.background = ui.visuals().code_bg_color;
            job.append(&r[..end], 0.0, fmt);
            rest = r.get(end + 1..).unwrap_or("");
        }
    }
    job
}

fn code_block(ui: &mut egui::Ui, code: &str) {
    egui::Frame::new()
        .fill(ui.visuals().code_bg_color)
        .corner_radius(6.0)
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(RichText::new(code).monospace())
                    .wrap()
                    .selectable(true),
            );
        });
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
