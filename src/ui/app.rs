//! Application state and the top-level frame layout. Behaviour is split
//! across the sibling modules (editing, commands, files, keyboard, …), each
//! adding methods to [`App`].

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use egui::Context;

use super::selection::Selection;
use super::util::file_name;
use crate::ai;
use crate::model::filter::{Op, ViewState};
use crate::model::{self, Book, Cell, Pos};

/// egui ids of the text fields the keyboard handling needs to know about.
pub const CELL_EDITOR_ID: &str = "cell_editor";
pub const BAR_EDITOR_ID: &str = "formula_bar_editor";
pub const SEARCH_ID: &str = "search_box";
pub use crate::model::{MAX_COLS, MAX_ROWS};

pub struct EditState {
    pub pos: Pos,
    pub buf: String,
    /// Editing happens in the formula bar rather than in the cell.
    pub in_bar: bool,
    /// Move the text cursor to the end on the next frame.
    pub place_cursor: bool,
    /// While pointing at cells to build a formula: (anchor cell, buffer length before the ref).
    pub pointing: Option<(Pos, usize)>,
}

pub(crate) struct ClipData {
    pub(crate) text: String,
    pub(crate) origin: Pos,
    pub(crate) raws: Vec<Vec<Option<Cell>>>,
    pub(crate) sheet: usize,
    pub(crate) source: Selection,
    pub(crate) cut: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Pending {
    New,
    Open,
    /// Open this file (from Finder or a drop), once unsaved changes are dealt with.
    OpenFile(PathBuf),
    Close,
}

pub struct FilterPopup {
    pub col: u32,
    pub conds: [(Op, String); 2],
    pub use_second: bool,
    pub values: Vec<(String, bool)>,
    pub value_search: String,
    pub truncated: bool,
    /// Where to open the window (below the header button), and a fresh id per opening.
    pub anchor: Option<egui::Pos2>,
    pub serial: u64,
}

pub struct App {
    pub book: Book,
    pub views: Vec<ViewState>,
    pub active: usize,
    pub sel: Selection,
    pub edit: Option<EditState>,
    pub scroll_to_cursor: bool,
    pub filter_buttons: bool,
    pub filter_popup: Option<FilterPopup>,
    /// The chart selected on the sheet (index into the active sheet's charts).
    pub selected_chart: Option<usize>,
    pub(crate) chart_drag: Option<super::chart_layer::ChartDrag>,
    pub(crate) chart_dialog: Option<super::chart_layer::ChartDialog>,
    pub context_target: Option<(Option<u32>, Option<u32>)>,
    pub resizing: Option<(u32, f32, f32)>,
    /// Bumped on every data change; used to cache selection stats.
    pub data_version: u64,

    pub(crate) show_search: bool,
    pub(crate) focus_search: bool,
    pub(crate) name_box: Option<String>,
    pub(crate) rename: Option<(usize, String)>,
    pub(crate) status: Option<(String, Instant, bool)>,
    pub(crate) pending: Option<Pending>,
    pub(crate) allow_close: bool,
    pub(crate) loading: Option<super::files::Loading>,
    pub(crate) clip: Option<ClipData>,
    pub(crate) title: String,
    pub(crate) stats_cache: Option<((usize, Selection, u64), String)>,

    pub ai: super::ai_panel::AiPanel,
    pub rt: tokio::runtime::Runtime,
    pub http: reqwest::Client,
    pub ai_tx: Sender<ai::Response>,
    pub ai_rx: Receiver<ai::Response>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = true);
        crate::platform::set_repaint_context(&cc.egui_ctx);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        let (ai_tx, ai_rx) = channel();
        let mut app = Self {
            book: Book::new_blank(),
            views: vec![ViewState::default()],
            active: 0,
            sel: Selection::single((0, 0)),
            edit: None,
            scroll_to_cursor: false,
            filter_buttons: false,
            filter_popup: None,
            selected_chart: None,
            chart_drag: None,
            chart_dialog: None,
            context_target: None,
            resizing: None,
            data_version: 0,
            show_search: false,
            focus_search: false,
            name_box: None,
            rename: None,
            status: None,
            pending: None,
            allow_close: false,
            loading: None,
            clip: None,
            title: String::new(),
            stats_cache: None,
            ai: super::ai_panel::AiPanel::new(),
            rt,
            http: reqwest::Client::new(),
            ai_tx,
            ai_rx,
        };
        if let Some(path) = open {
            app.start_load(path, &cc.egui_ctx);
        }
        app
    }

    pub(crate) fn sheet(&self) -> &model::Sheet {
        &self.book.sheets[self.active]
    }

    pub(crate) fn view(&self) -> &ViewState {
        &self.views[self.active]
    }

    pub(crate) fn notify(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now(), false));
    }

    pub(crate) fn error(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now(), true));
    }

    /// Call after any change to cell data.
    pub(crate) fn data_changed(&mut self) {
        self.data_version += 1;
        let s = &self.book.sheets[self.active];
        self.views[self.active].refresh(s);
    }

    pub(crate) fn set_book(&mut self, book: Book) {
        self.views = book.sheets.iter().map(|_| ViewState::default()).collect();
        self.book = book;
        self.active = 0;
        self.sel = Selection::single((0, 0));
        self.edit = None;
        self.filter_popup = None;
        self.selected_chart = None;
        self.chart_dialog = None;
        self.clip = None;
        self.data_version += 1;
        self.scroll_to_cursor = true;
    }

    pub(crate) fn switch_sheet(&mut self, idx: usize) {
        if idx == self.active || idx >= self.book.sheets.len() {
            return;
        }
        self.commit_edit((0, 0));
        self.active = idx;
        self.sel = Selection::single((0, 0));
        self.filter_popup = None;
        self.selected_chart = None;
        self.data_changed();
        self.scroll_to_cursor = true;
    }

    /// The copied/cut range on the active sheet, for the outline in the grid.
    pub(crate) fn clip_outline(&self) -> Option<Selection> {
        let clip = self.clip.as_ref()?;
        (clip.sheet == self.active).then_some(clip.source)
    }

    /// Number of rows in the current view (visible rows when filtered).
    pub(crate) fn view_rows(&self) -> u32 {
        match &self.view().rows {
            Some(rows) => rows.len() as u32,
            None => (self.sheet().n_rows + 200)
                .max(1000)
                .max(self.sel.cursor.0 + 50)
                .min(MAX_ROWS),
        }
    }

    pub(crate) fn view_cols(&self) -> u32 {
        (self.sheet().n_cols + 10)
            .max(26)
            .max(self.sel.cursor.1 + 5)
            .min(MAX_COLS)
    }

    pub(crate) fn row_at(&self, view_idx: u32) -> Option<u32> {
        match &self.view().rows {
            Some(rows) => rows.get(view_idx as usize).copied(),
            None => Some(view_idx),
        }
    }

    pub(crate) fn view_index(&self, row: u32) -> Option<u32> {
        match &self.view().rows {
            Some(rows) => rows.binary_search(&row).ok().map(|i| i as u32),
            None => Some(row),
        }
    }

    /// Move the cursor by (rows, cols) in view space.
    pub(crate) fn move_cursor(&mut self, dr: i64, dc: i64, extend: bool) {
        // Step off a merged cell from its far edge, so arrows skip over it.
        let (mut r, mut c) = self.sel.cursor;
        if let Some(((r0, c0), (r1, c1))) = self.sheet().merge_at((r, c)) {
            r = if dr > 0 { r1 } else { r0 };
            c = if dc > 0 { c1 } else { c0 };
        }
        let vi = self.view_index(r).unwrap_or(0) as i64;
        let max_vi = self.view_rows().saturating_sub(1) as i64
            + if self.view().is_filtered() { 0 } else { 1000 };
        let nvi = (vi + dr).clamp(0, max_vi.max(0));
        let nr = self.row_at(nvi as u32).unwrap_or(r).min(MAX_ROWS - 1);
        let nc = (c as i64 + dc).clamp(0, MAX_COLS as i64 - 1) as u32;
        self.set_cursor((nr, nc), extend);
    }

    pub(crate) fn set_cursor(&mut self, pos: Pos, extend: bool) {
        if extend {
            self.sel.cursor = pos;
        } else {
            self.sel = Selection::single(pos);
        }
        self.snap_to_merges();
        self.scroll_to_cursor = true;
    }

    /// Grow the selection over any merged range it partly covers, and keep
    /// the active cell on a merge's top-left cell (where its value lives).
    pub(crate) fn snap_to_merges(&mut self) {
        let merges = &self.book.sheets[self.active].merges;
        if merges.is_empty() {
            return;
        }
        let (mut a, mut b) = self.sel.range();
        loop {
            let mut grew = false;
            for &m in merges {
                if model::merges_overlap(m, (a, b)) {
                    let (na, nb) = (
                        (a.0.min(m.0.0), a.1.min(m.0.1)),
                        (b.0.max(m.1.0), b.1.max(m.1.1)),
                    );
                    grew |= (na, nb) != (a, b);
                    (a, b) = (na, nb);
                }
            }
            if !grew {
                break;
            }
        }
        let mut cursor = self.sel.cursor;
        if let Some(m) = merges.iter().find(|&&m| model::merge_contains(m, cursor)) {
            cursor = m.0;
        }
        // The cursor must sit on an edge of the range for (anchor, cursor) to describe it.
        if cursor.0 != a.0 && cursor.0 != b.0 {
            cursor.0 = a.0;
        }
        if cursor.1 != a.1 && cursor.1 != b.1 {
            cursor.1 = a.1;
        }
        let anchor = (
            if cursor.0 == a.0 { b.0 } else { a.0 },
            if cursor.1 == a.1 { b.1 } else { a.1 },
        );
        self.sel = Selection { anchor, cursor };
    }

    pub(crate) fn update_title(&mut self, ctx: &Context) {
        let name = self
            .book
            .path
            .as_deref()
            .map(file_name)
            .unwrap_or_else(|| "Untitled".into());
        let title = format!(
            "{}{} — LiteSheet",
            if self.book.dirty { "• " } else { "" },
            name
        );
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_loading();
        self.poll_ai();

        if ctx.input(|i| i.viewport().close_requested()) && self.book.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.commit_edit((0, 0));
            self.pending = Some(Pending::Close);
        }
        // Files dropped on the window or opened from Finder.
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect::<Vec<_>>()
        });
        let mut incoming = crate::platform::take_opened_files();
        incoming.extend(dropped);
        if let Some(path) = incoming.first().cloned() {
            if incoming.len() > 1 {
                self.notify("LiteSheet opens one workbook at a time; opening the first");
            }
            if self.loading.is_none() {
                self.request(Pending::OpenFile(path), &ctx);
            }
        }

        if self.pending.is_none() && self.loading.is_none() {
            self.shortcuts(&ctx);
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            self.toolbar(ui);
            ui.add_space(3.0);
        });
        egui::Panel::top("formula_bar").show(ui, |ui| {
            ui.add_space(3.0);
            self.formula_bar(ui);
            if self.show_search {
                ui.add_space(2.0);
                self.search_bar(ui);
            }
            ui.add_space(3.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("tabs").show(ui, |ui| {
            ui.add_space(2.0);
            self.sheet_tabs(ui);
            ui.add_space(2.0);
        });
        if self.ai.open {
            egui::Panel::right("ai_panel")
                .resizable(true)
                .default_size(340.0)
                .size_range(260.0..=700.0)
                .show(ui, |ui| self.ai_panel_ui(ui));
        }
        if self.loading.is_some() {
            egui::CentralPanel::default().show(ui, |ui| self.loading_ui(ui));
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.grid_ui(ui));
        }

        self.filter_window(&ctx);
        self.chart_window(&ctx);
        self.confirm_dialog(&ctx);
        self.update_title(&ctx);
    }
}
