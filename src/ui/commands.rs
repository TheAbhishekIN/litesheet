//! Commands that change cell data: clear, clipboard, fill, sort, undo/redo
//! and number formats.

use egui::Context;

use super::app::{App, ClipData};
use super::selection::Selection;
use crate::model::{self, Cell, Pos, Value, interpret_input, refs};

impl App {
    pub(crate) fn sel_positions(&self) -> Vec<Pos> {
        let ((r0, c0), (r1, c1)) = self.sel.range();
        let s = self.sheet();
        // Only touch cells that exist when the selection is huge.
        if (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64 > 200_000 {
            return s
                .cells
                .keys()
                .copied()
                .filter(|p| self.sel.contains(*p))
                .collect();
        }
        (r0..=r1)
            .flat_map(|r| (c0..=c1).map(move |c| (r, c)))
            .collect()
    }

    pub(crate) fn clear_selection(&mut self) {
        let edits = self
            .sel_positions()
            .into_iter()
            .filter(|p| self.sheet().cells.contains_key(p))
            .map(|p| (self.active, p, None))
            .collect();
        self.book.apply("Clear", edits);
        self.data_changed();
    }

    pub(crate) fn selection_bounds_clamped(&self) -> (Pos, Pos) {
        let ((r0, c0), (r1, c1)) = self.sel.range();
        let s = self.sheet();
        let r1 = r1.min(s.n_rows.max(r0 + 1) - 1);
        let c1 = c1.min(s.n_cols.max(c0 + 1) - 1);
        ((r0, c0), (r1, c1))
    }

    pub(crate) fn copy(&mut self, ctx: &Context, cut: bool) {
        let ((r0, c0), (r1, c1)) = self.selection_bounds_clamped();
        let s = self.sheet();
        let mut text = String::new();
        let mut raws = Vec::new();
        for r in r0..=r1 {
            if self.view().is_filtered() && self.view_index(r).is_none() {
                continue;
            }
            let mut row = Vec::new();
            for c in c0..=c1 {
                if c > c0 {
                    text.push('\t');
                }
                text.push_str(&s.display((r, c)).replace(['\t', '\n'], " "));
                row.push(s.cells.get(&(r, c)).cloned());
            }
            text.push('\n');
            raws.push(row);
        }
        ctx.copy_text(text.clone());
        let n = raws.len() * (c1 - c0 + 1) as usize;
        self.clip = Some(ClipData {
            text,
            origin: (r0, c0),
            raws,
            sheet: self.active,
            source: self.sel,
            cut,
        });
        self.notify(if cut {
            format!("Cut {n} cells")
        } else {
            format!("Copied {n} cells")
        });
    }

    pub(crate) fn paste(&mut self, text: &str) {
        let dest = self.sel.range().0;
        let mut edits = Vec::new();
        let internal = self
            .clip
            .as_ref()
            .filter(|c| c.text.trim_end() == text.trim_end().replace("\r\n", "\n"));
        let mut cut_source = None;
        let (rows, cols) = if let Some(clip) = internal {
            let (dr, dc) = (
                dest.0 as i64 - clip.origin.0 as i64,
                dest.1 as i64 - clip.origin.1 as i64,
            );
            for (i, row) in clip.raws.iter().enumerate() {
                for (j, cell) in row.iter().enumerate() {
                    let cell = cell.clone().map(|mut c| {
                        if c.is_formula() && !clip.cut {
                            c.raw = refs::shift_formula(&c.raw, dr, dc);
                        }
                        c
                    });
                    edits.push((self.active, (dest.0 + i as u32, dest.1 + j as u32), cell));
                }
            }
            cut_source = clip.cut.then_some((clip.sheet, clip.source));
            (clip.raws.len(), clip.raws.first().map_or(0, |r| r.len()))
        } else {
            let text = text.replace("\r\n", "\n");
            let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
            let mut w = 0;
            for (i, line) in lines.iter().enumerate() {
                let fields: Vec<&str> = line.split('\t').collect();
                w = w.max(fields.len());
                for (j, f) in fields.iter().enumerate() {
                    let pos = (dest.0 + i as u32, dest.1 + j as u32);
                    edits.push((self.active, pos, interpret_input(f, model::NumFmt::General)));
                }
            }
            (lines.len(), w)
        };
        // A single copied cell fills the whole selection.
        if rows == 1 && cols == 1 && !self.sel.is_single() {
            let src = edits.pop().and_then(|e| e.2);
            edits.clear();
            for p in self.sel_positions() {
                let cell = src.clone().map(|mut c| {
                    if c.is_formula() {
                        c.raw = refs::shift_formula(
                            &c.raw,
                            p.0 as i64 - dest.0 as i64,
                            p.1 as i64 - dest.1 as i64,
                        );
                    }
                    c
                });
                edits.push((self.active, p, cell));
            }
        } else if rows > 0 && cols > 0 {
            self.sel = Selection {
                anchor: dest,
                cursor: (dest.0 + rows as u32 - 1, dest.1 + cols as u32 - 1),
            };
        }
        if let Some((sheet, src_sel)) = cut_source {
            let targets: std::collections::HashSet<Pos> = edits.iter().map(|e| e.1).collect();
            let ((r0, c0), (r1, c1)) = src_sel.range();
            for r in r0..=r1 {
                for c in c0..=c1 {
                    if sheet != self.active || !targets.contains(&(r, c)) {
                        edits.push((sheet, (r, c), None));
                    }
                }
            }
            self.clip = None;
        }
        self.book.apply("Paste", edits);
        self.data_changed();
    }

    /// Fill the first row (down) or column (right) of the selection across it.
    pub(crate) fn fill(&mut self, down: bool) {
        let ((r0, c0), (r1, c1)) = self.sel.range();
        let s = self.sheet();
        let mut edits = Vec::new();
        if down {
            for c in c0..=c1 {
                let src = s.cells.get(&(r0, c));
                for r in r0 + 1..=r1 {
                    let cell = src.cloned().map(|mut cell| {
                        cell.raw = refs::shift_formula(&cell.raw, (r - r0) as i64, 0);
                        cell
                    });
                    edits.push((self.active, (r, c), cell));
                }
            }
        } else {
            for r in r0..=r1 {
                let src = s.cells.get(&(r, c0));
                for c in c0 + 1..=c1 {
                    let cell = src.cloned().map(|mut cell| {
                        cell.raw = refs::shift_formula(&cell.raw, 0, (c - c0) as i64);
                        cell
                    });
                    edits.push((self.active, (r, c), cell));
                }
            }
        }
        self.book
            .apply(if down { "Fill down" } else { "Fill right" }, edits);
        self.data_changed();
    }

    pub(crate) fn sort_by(&mut self, col: u32, ascending: bool) {
        let s = self.sheet();
        let ((r0, _), (r1, _)) = self.sel.range();
        let header = self.view().header_row as u32;
        let (first, last) = if r1 > r0 && r1 < s.n_rows {
            (r0, r1)
        } else {
            (header, s.n_rows.saturating_sub(1))
        };
        if s.n_rows == 0 || last <= first {
            return;
        }
        let rows: Vec<u32> = (first..=last).collect();
        let mut order = rows.clone();
        order.sort_by(|&a, &b| {
            let (va, vb) = (s.value((a, col)), s.value((b, col)));
            // Blanks always sink to the bottom.
            match (va, vb) {
                (Value::Empty, Value::Empty) => std::cmp::Ordering::Equal,
                (Value::Empty, _) => std::cmp::Ordering::Greater,
                (_, Value::Empty) => std::cmp::Ordering::Less,
                _ => {
                    let o = model::compare_values(va, vb);
                    if ascending { o } else { o.reverse() }
                }
            }
        });
        let edits = model::move_rows_edits(s, self.active, &rows, &order, 0..s.n_cols);
        self.book.apply(
            if ascending {
                "Sort ascending"
            } else {
                "Sort descending"
            },
            edits,
        );
        self.data_changed();
        self.notify(format!(
            "Sorted rows {}–{} by column {}",
            first + 1,
            last + 1,
            refs::col_name(col)
        ));
    }

    pub(crate) fn undo(&mut self) {
        self.commit_edit((0, 0));
        if let Some((sheet, cells)) = self.book.undo() {
            self.after_history(sheet, cells);
        }
    }

    pub(crate) fn redo(&mut self) {
        self.commit_edit((0, 0));
        if let Some((sheet, cells)) = self.book.redo() {
            self.after_history(sheet, cells);
        }
    }

    pub(crate) fn after_history(&mut self, sheet: usize, cells: Vec<Pos>) {
        if sheet != self.active && sheet < self.book.sheets.len() {
            self.active = sheet;
        }
        if let (Some(min_r), Some(min_c), Some(max_r), Some(max_c)) = (
            cells.iter().map(|p| p.0).min(),
            cells.iter().map(|p| p.1).min(),
            cells.iter().map(|p| p.0).max(),
            cells.iter().map(|p| p.1).max(),
        ) {
            self.sel = Selection {
                anchor: (min_r, min_c),
                cursor: (max_r, max_c),
            };
            if cells.len() == 1 {
                self.sel = Selection::single((min_r, min_c));
            }
            self.scroll_to_cursor = true;
        }
        self.data_changed();
    }

    pub(crate) fn set_format(&mut self, fmt: model::NumFmt) {
        let edits = self
            .sel_positions()
            .into_iter()
            .filter_map(|p| {
                let cell = self.sheet().cells.get(&p)?;
                (cell.fmt != fmt).then(|| {
                    (
                        self.active,
                        p,
                        Some(Cell {
                            raw: cell.raw.clone(),
                            fmt,
                        }),
                    )
                })
            })
            .collect();
        self.book.apply("Format", edits);
        self.data_changed();
    }
}
