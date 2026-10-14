//! Several open workbooks, shown as tabs above the toolbar.
//!
//! The active workbook's state lives directly on [`App`], so the rest of the
//! UI never has to think about documents. The others are parked in
//! `App::docs`; switching swaps the two sets of fields.

use std::path::{Path, PathBuf};

use egui::{Context, RichText};

use super::app::{App, EditState, FilterPopup};
use super::chart_layer::{ChartDialog, ChartDrag};
use super::files::Loading;
use super::selection::Selection;
use super::util::file_name;
use crate::model::Book;
use crate::model::filter::ViewState;

/// One open workbook and its view state. While a document is the active
/// one, its slot in `App::docs` holds stale placeholder values.
pub(crate) struct Doc {
    pub(crate) id: u64,
    book: Book,
    views: Vec<ViewState>,
    active: usize,
    sel: Selection,
    edit: Option<EditState>,
    scroll_to_cursor: bool,
    filter_popup: Option<FilterPopup>,
    selected_chart: Option<usize>,
    chart_drag: Option<ChartDrag>,
    chart_dialog: Option<ChartDialog>,
    context_target: Option<(Option<u32>, Option<u32>)>,
    resizing: Option<(u32, f32, f32)>,
    name_box: Option<String>,
    rename: Option<(usize, String)>,
    loading: Option<Loading>,
}

impl Doc {
    pub(crate) fn new(id: u64, book: Book) -> Self {
        Self {
            id,
            views: book.sheets.iter().map(|_| ViewState::default()).collect(),
            book,
            active: 0,
            sel: Selection::single((0, 0)),
            edit: None,
            scroll_to_cursor: true,
            filter_popup: None,
            selected_chart: None,
            chart_drag: None,
            chart_dialog: None,
            context_target: None,
            resizing: None,
            name_box: None,
            rename: None,
            loading: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    fn path(&self) -> Option<&Path> {
        match &self.loading {
            Some(l) => Some(&l.path),
            None => self.book.path.as_deref(),
        }
    }
}

/// Swap the listed fields between the app and a parked document.
macro_rules! swap_fields {
    ($app:expr, $doc:expr, $($field:ident),* $(,)?) => {
        $(std::mem::swap(&mut $app.$field, &mut $doc.$field);)*
    };
}

impl App {
    fn swap_with(&mut self, i: usize) {
        let doc = &mut self.docs[i];
        swap_fields!(
            self,
            doc,
            book,
            views,
            active,
            sel,
            edit,
            scroll_to_cursor,
            filter_popup,
            selected_chart,
            chart_drag,
            chart_dialog,
            context_target,
            resizing,
            name_box,
            rename,
            loading,
        );
    }

    /// The id of the active document.
    pub(crate) fn doc_id(&self) -> u64 {
        self.docs[self.current].id
    }

    fn next_id(&mut self) -> u64 {
        self.next_doc_id += 1;
        self.next_doc_id
    }

    pub(crate) fn switch_doc(&mut self, i: usize) {
        if i == self.current || i >= self.docs.len() {
            return;
        }
        self.commit_edit((0, 0));
        self.swap_with(self.current); // park the active document
        self.swap_with(i); // and bring in the new one
        self.current = i;
        self.data_version += 1;
        self.stats_cache = None;
        self.scroll_to_cursor = true;
    }

    /// Open a blank workbook in a new tab.
    pub(crate) fn new_doc(&mut self) {
        let id = self.next_id();
        self.docs.push(Doc::new(id, Book::new_blank()));
        self.switch_doc(self.docs.len() - 1);
    }

    /// An untitled workbook nobody has typed into yet: opening a file
    /// replaces it instead of adding a tab next to it.
    fn is_pristine(&self) -> bool {
        self.loading.is_none()
            && self.book.path.is_none()
            && !self.book.dirty
            && !self.book.can_undo()
            && self.book.sheets.iter().all(|s| s.cells.is_empty())
    }

    /// Open a file in its own tab (or switch to it if it's already open).
    pub(crate) fn open_path(&mut self, path: PathBuf, ctx: &Context) {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let same = |p: Option<&Path>| {
            p.is_some_and(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()) == key)
        };
        let current_path = self
            .loading
            .as_ref()
            .map(|l| l.path.as_path())
            .or(self.book.path.as_deref());
        if same(current_path) {
            return;
        }
        if let Some(i) =
            (0..self.docs.len()).find(|&i| i != self.current && same(self.docs[i].path()))
        {
            self.switch_doc(i);
            return;
        }
        if !self.is_pristine() {
            let id = self.next_id();
            self.docs.push(Doc::new(id, Book::new_blank()));
            self.switch_doc(self.docs.len() - 1);
        }
        self.start_load(path, ctx);
    }

    /// Close the active tab without asking about unsaved changes.
    pub(crate) fn close_doc(&mut self) {
        if self.docs.len() == 1 {
            self.loading = None;
            self.set_book(Book::new_blank());
            return;
        }
        let closing = self.current;
        let next = if closing + 1 < self.docs.len() {
            closing + 1
        } else {
            closing - 1
        };
        self.edit = None;
        self.swap_with(next);
        // The closed workbook now sits in `next`'s slot; drop it.
        let id = self.docs[next].id;
        self.docs[next] = Doc::new(id, Book::new_blank());
        self.docs.remove(closing);
        self.current = if next > closing { next - 1 } else { next };
        self.data_version += 1;
        self.stats_cache = None;
        self.scroll_to_cursor = true;
        if self
            .clip
            .as_ref()
            .is_some_and(|c| !self.docs.iter().any(|d| d.id == c.doc))
        {
            self.clip = None;
        }
    }

    /// Whether a document (active or parked) has changes the user hasn't
    /// saved or chosen to discard.
    pub(crate) fn doc_needs_saving(&self, i: usize) -> bool {
        let dirty = if i == self.current {
            self.book.dirty
        } else {
            self.docs[i].book.dirty
        };
        dirty && !self.discarded.contains(&self.docs[i].id)
    }

    /// Finish loads that completed in background tabs.
    pub(crate) fn poll_parked_loads(&mut self) {
        let mut i = 0;
        while i < self.docs.len() {
            let done = (i != self.current)
                .then(|| self.docs[i].loading.as_ref().and_then(Loading::poll))
                .flatten();
            match done {
                Some(Ok(book)) => {
                    let doc = &mut self.docs[i];
                    let name = doc.path().map(file_name).unwrap_or_default();
                    *doc = Doc::new(doc.id, book);
                    self.notify(format!("Opened {name}"));
                }
                Some(Err(e)) => {
                    self.docs.remove(i);
                    if i < self.current {
                        self.current -= 1;
                    }
                    self.error(e);
                    continue;
                }
                None => {}
            }
            i += 1;
        }
    }

    /// The tab strip, shown once more than one workbook is open.
    pub(crate) fn doc_tabs(&mut self, ui: &mut egui::Ui) {
        let mut switch = None;
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("doc_tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for i in 0..self.docs.len() {
                        let (path, dirty, loading) = if i == self.current {
                            let path = match &self.loading {
                                Some(l) => Some(l.path.clone()),
                                None => self.book.path.clone(),
                            };
                            (path, self.book.dirty, self.loading.is_some())
                        } else {
                            let d = &self.docs[i];
                            (
                                d.path().map(Path::to_path_buf),
                                d.book.dirty,
                                d.loading.is_some(),
                            )
                        };
                        let name = path
                            .as_deref()
                            .map(file_name)
                            .unwrap_or_else(|| "Untitled".into());
                        let selected = i == self.current;
                        let frame = egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .corner_radius(6.0)
                            .fill(if selected {
                                ui.visuals().selection.bg_fill
                            } else {
                                ui.visuals().faint_bg_color
                            });
                        frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if loading {
                                    ui.add(egui::Spinner::new().size(11.0));
                                }
                                let mut text = RichText::new(if dirty {
                                    format!("• {name}")
                                } else {
                                    name.clone()
                                });
                                if selected {
                                    text = text.color(ui.visuals().selection.stroke.color);
                                }
                                let label = ui.add(
                                    egui::Label::new(text)
                                        .truncate()
                                        .sense(egui::Sense::click()),
                                );
                                let label = match &path {
                                    Some(p) => label.on_hover_text(p.display().to_string()),
                                    None => label,
                                };
                                if label.clicked() {
                                    switch = Some(i);
                                }
                                if label.middle_clicked() {
                                    close = Some(i);
                                }
                                let x = ui
                                    .add(egui::Button::new(RichText::new("×").small()).frame(false))
                                    .on_hover_text("Close");
                                if x.clicked() {
                                    close = Some(i);
                                }
                            });
                        });
                    }
                    if ui
                        .add(egui::Button::new("+").frame(false))
                        .on_hover_text("New workbook")
                        .clicked()
                    {
                        self.new_doc();
                    }
                });
            });
        if let Some(i) = close {
            self.switch_doc(i);
            self.request_close_doc(ui.ctx());
        } else if let Some(i) = switch {
            self.switch_doc(i);
        }
    }
}
