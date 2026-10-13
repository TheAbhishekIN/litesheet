//! Opening, saving and exporting files, plus the unsaved-changes dialog.

use std::path::PathBuf;
use std::sync::mpsc::channel;

use egui::{Context, Id};

use super::app::{App, Pending};
use super::util::file_name;
use crate::io;
use crate::model::Book;

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
        let p = path.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = io::load(&p).map_err(|e| format!("{e:#}"));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.loading = Some((path, rx));
    }

    pub(crate) fn poll_loading(&mut self) {
        let Some((path, rx)) = &self.loading else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(book)) => {
                let name = file_name(path);
                let cells: usize = book.sheets.iter().map(|s| s.cells.len()).sum();
                self.set_book(book);
                self.loading = None;
                self.notify(format!("Opened {name} ({cells} cells)"));
                if std::env::var_os("LITESHEET_LOG").is_some() {
                    eprintln!("litesheet: opened {name} ({cells} cells)");
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
