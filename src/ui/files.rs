//! Opening, saving and exporting files, plus the unsaved-changes dialog.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{Context, Id, RichText};

use super::app::{App, Pending};
use super::util::file_name;
use crate::io;
use crate::model::Book;

/// A file being read on a background thread.
pub(crate) struct Loading {
    pub(crate) path: PathBuf,
    rx: Receiver<Result<Book, String>>,
    started: Instant,
    bytes: Option<u64>,
    /// What the loader is doing right now, e.g. "Reading sheet 2 of 3: Data…".
    step: Arc<Mutex<String>>,
}

#[cfg(test)]
impl Loading {
    /// A load that never finishes, to look at the loading screen.
    pub(crate) fn stalled(path: PathBuf) -> (Self, std::sync::mpsc::Sender<Result<Book, String>>) {
        let (tx, rx) = channel();
        let loading = Self {
            path,
            rx,
            started: Instant::now(),
            bytes: Some(9_514_766),
            step: Arc::new(Mutex::new("Reading sheet 1 of 2: Data…".into())),
        };
        (loading, tx)
    }
}

impl App {
    pub(crate) fn request(&mut self, action: Pending, ctx: &Context) {
        self.commit_edit((0, 0));
        if self.book.dirty {
            self.pending = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    pub(crate) fn perform(&mut self, action: Pending, ctx: &Context) {
        match action {
            Pending::New => self.set_book(Book::new_blank()),
            Pending::Open => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(
                        "Spreadsheets",
                        &["xlsx", "xlsm", "xls", "ods", "csv", "tsv"],
                    )
                    .add_filter("All files", &["*"])
                    .pick_file()
                {
                    self.start_load(path, ctx);
                }
            }
            Pending::OpenFile(path) => self.start_load(path, ctx),
            Pending::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub(crate) fn start_load(&mut self, path: PathBuf, ctx: &Context) {
        let (tx, rx) = channel();
        let step = Arc::new(Mutex::new(String::from("Starting…")));
        let (p, thread_step, ctx) = (path.clone(), step.clone(), ctx.clone());
        std::thread::spawn(move || {
            let progress = |msg: &str| {
                if let Ok(mut s) = thread_step.lock() {
                    *s = msg.to_string();
                }
                ctx.request_repaint();
            };
            let r = io::load_with_progress(&p, &progress).map_err(|e| format!("{e:#}"));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.loading = Some(Loading {
            bytes: std::fs::metadata(&path).ok().map(|m| m.len()),
            path,
            rx,
            started: Instant::now(),
            step,
        });
    }

    pub(crate) fn poll_loading(&mut self) {
        let Some(loading) = &self.loading else {
            return;
        };
        match loading.rx.try_recv() {
            Ok(Ok(book)) => {
                let name = file_name(&loading.path);
                let secs = loading.started.elapsed().as_secs_f32();
                let cells: usize = book.sheets.iter().map(|s| s.cells.len()).sum();
                self.set_book(book);
                self.loading = None;
                self.notify(format!("Opened {name} ({cells} cells)"));
                if std::env::var_os("LITESHEET_LOG").is_some() {
                    eprintln!("litesheet: opened {name} ({cells} cells) in {secs:.1}s");
                }
            }
            Ok(Err(e)) => {
                self.loading = None;
                self.error(e);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => self.loading = None,
        }
    }

    /// The screen shown in place of the grid while a file loads.
    pub(crate) fn loading_ui(&mut self, ui: &mut egui::Ui) {
        let Some(loading) = &self.loading else {
            return;
        };
        let name = file_name(&loading.path);
        let step = loading.step.lock().map(|s| s.clone()).unwrap_or_default();
        let mut details = Vec::new();
        if let Some(bytes) = loading.bytes {
            details.push(human_size(bytes));
        }
        let secs = loading.started.elapsed().as_secs();
        if secs > 0 {
            details.push(format!("{secs} s"));
        }
        let mut cancel = false;
        let card = egui::Rect::from_center_size(ui.max_rect().center(), egui::vec2(400.0, 210.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
            egui::Frame::group(ui.style())
                .fill(ui.visuals().window_fill)
                .corner_radius(10.0)
                .inner_margin(24.0)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Spinner::new().size(30.0));
                        ui.add_space(12.0);
                        ui.add(
                            egui::Label::new(RichText::new(format!("Opening {name}")).heading())
                                .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.label(RichText::new(step).weak());
                        if !details.is_empty() {
                            ui.label(RichText::new(details.join(" · ")).weak());
                        }
                        ui.add_space(12.0);
                        cancel = ui.button("Cancel").clicked();
                    });
                });
        });
        if cancel {
            self.loading = None;
            self.notify(format!("Stopped opening {name}"));
        }
    }

    /// Returns true if saved.
    pub(crate) fn save(&mut self, save_as: bool) -> bool {
        self.commit_edit((0, 0));
        let path = match (&self.book.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let stem = self
                    .book
                    .path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .and_then(|s| s.to_str())
                    .unwrap_or("Untitled")
                    .to_string();
                match rfd::FileDialog::new()
                    .set_file_name(format!("{stem}.xlsx"))
                    .add_filter("Excel Workbook", &["xlsx"])
                    .add_filter("CSV (current sheet)", &["csv"])
                    .save_file()
                {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        match io::save(&self.book, self.active, &path) {
            Ok(()) => {
                let mut msg = format!("Saved {}", file_name(&path));
                if io::is_csv(&path) && self.book.sheets.len() > 1 {
                    msg.push_str(" (CSV holds only the current sheet)");
                }
                self.notify(msg);
                self.book.path = Some(path);
                self.book.dirty = false;
                true
            }
            Err(e) => {
                self.error(format!("{e:#}"));
                false
            }
        }
    }

    pub(crate) fn export_csv(&mut self) {
        self.commit_edit((0, 0));
        let name = format!("{}.csv", self.sheet().name);
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(name)
            .add_filter("CSV", &["csv"])
            .save_file()
        {
            match io::save(&self.book, self.active, &path) {
                Ok(()) => self.notify(format!("Exported {}", file_name(&path))),
                Err(e) => self.error(format!("{e:#}")),
            }
        }
    }

    pub(crate) fn confirm_dialog(&mut self, ctx: &Context) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        let name = self
            .book
            .path
            .as_deref()
            .map(file_name)
            .unwrap_or_else(|| "Untitled".into());
        let mut choice = None;
        egui::Modal::new(Id::new("confirm_save")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading(format!("Save changes to \"{name}\"?"));
            ui.label("Your changes will be lost if you don't save them.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't Save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                self.pending = None;
                if self.save(false) {
                    self.perform(pending, ctx);
                }
            }
            Some(1) => {
                self.pending = None;
                self.book.dirty = false;
                self.perform(pending, ctx);
            }
            Some(2) => self.pending = None,
            _ => {}
        }
    }
}

fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    match bytes as f64 {
        b if b >= MB => format!("{:.1} MB", b / MB),
        b if b >= 1024.0 => format!("{:.0} KB", b / 1024.0),
        b => format!("{b} bytes"),
    }
}
