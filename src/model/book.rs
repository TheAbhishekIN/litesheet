//! The workbook: sheets, undo/redo history, and the bridge to the
//! formualizer calculation engine.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::Timelike;
use formualizer::LiteralValue;
use formualizer::workbook::{Workbook as Engine, WorkbookConfig};

use super::format::date_to_serial;
use super::refs::{self, Axis, Structural};
use super::sheet::{Freeze, Merge, merge_contains, merges_overlap};
use super::{Cell, Chart, HAlign, NumFmt, Pos, Sheet, Value, interpret_input, parse_literal};

#[derive(Clone, Debug)]
pub struct CellChange {
    pub sheet: usize,
    pub pos: Pos,
    pub old: Option<Cell>,
    pub new: Option<Cell>,
}

/// A change that isn't a cell edit but must be undone along with one.
#[derive(Clone, Debug)]
pub enum MetaChange {
    RenameSheet {
        sheet: usize,
        old: String,
        new: String,
    },
    ColWidths {
        sheet: usize,
        old: HashMap<u32, f32>,
        new: HashMap<u32, f32>,
    },
    Merges {
        sheet: usize,
        old: Vec<Merge>,
        new: Vec<Merge>,
    },
    Freeze {
        sheet: usize,
        old: Freeze,
        new: Freeze,
    },
    Charts {
        sheet: usize,
        old: Vec<Chart>,
        new: Vec<Chart>,
    },
}

struct HistoryEntry {
    label: String,
    /// Applied before `changes` when redoing, after them when undoing.
    meta: Vec<MetaChange>,
    changes: Vec<CellChange>,
}

pub struct Book {
    pub sheets: Vec<Sheet>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    engine: Engine,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

const MAX_UNDO: usize = 200;
/// Excel's sheet size.
pub const MAX_ROWS: u32 = 1_048_576;
pub const MAX_COLS: u32 = 16_384;

fn new_engine() -> Engine {
    let mut config = WorkbookConfig::interactive();
    // We keep our own undo history; the engine's change log would only cost memory.
    config.enable_changelog = false;
    Engine::new_with_config(config)
}

impl Book {
    pub fn new_blank() -> Self {
        let mut book = Self::empty();
        book.add_sheet("Sheet1");
        book
    }

    fn empty() -> Self {
        Self {
            sheets: Vec::new(),
            path: None,
            dirty: false,
            engine: new_engine(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Build a book from loaded sheet data (used by the file readers).
    pub fn from_sheets(sheets: Vec<(String, Vec<(Pos, Cell)>)>) -> Self {
        let mut book = Self::empty();
        for (name, cells) in sheets {
            let idx = book.add_sheet(&name);
            if let (Some(r), Some(c)) = (
                cells.iter().map(|x| x.0.0).max(),
                cells.iter().map(|x| x.0.1).max(),
            ) {
                book.reserve(idx, (r, c));
            }
            for (pos, cell) in cells {
                book.write_cell(idx, pos, Some(cell));
            }
            book.sheets[idx].recompute_extent();
        }
        if book.sheets.is_empty() {
            book.add_sheet("Sheet1");
        }
        book.recalc();
        book
    }

    pub fn add_sheet(&mut self, name: &str) -> usize {
        let name = self.unique_sheet_name(name);
        let _ = self.engine.add_sheet(&name);
        self.sheets.push(Sheet::new(name));
        self.sheets.len() - 1
    }

    pub fn unique_sheet_name(&self, base: &str) -> String {
        let base = if base.trim().is_empty() {
            "Sheet"
        } else {
            base.trim()
        };
        if !self
            .sheets
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(base))
        {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !self.sheets.iter().any(|s| s.name.eq_ignore_ascii_case(n)))
            .unwrap()
    }

    pub fn rename_sheet(&mut self, idx: usize, new_name: &str) -> Result<(), String> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err("Sheet name can't be empty".into());
        }
        if new_name.contains(['[', ']', ':', '*', '?', '/', '\\']) || new_name.len() > 31 {
            return Err("Sheet names can't contain [ ] : * ? / \\ or exceed 31 characters".into());
        }
        if self
            .sheets
            .iter()
            .enumerate()
            .any(|(i, s)| i != idx && s.name.eq_ignore_ascii_case(new_name))
        {
            return Err(format!("A sheet named \"{new_name}\" already exists"));
        }
        let old = self.sheets[idx].name.clone();
        if old == new_name {
            return Ok(());
        }
        let rename = MetaChange::RenameSheet {
            sheet: idx,
            old: old.clone(),
            new: new_name.to_string(),
        };
        self.apply_meta(&rename, false)?;

        // Point formulas in every sheet at the new name.
        let mut edits = Vec::new();
        for (si, sheet) in self.sheets.iter().enumerate() {
            for &pos in &sheet.formulas {
                let cell = &sheet.cells[&pos];
                let raw = refs::rename_sheet_refs(&cell.raw, &old, new_name);
                if raw != cell.raw {
                    edits.push((
                        si,
                        pos,
                        Some(Cell {
                            raw,
                            ..cell.clone()
                        }),
                    ));
                }
            }
        }
        let mut meta = vec![rename];
        for si in 0..self.sheets.len() {
            let old_charts = self.sheets[si].charts.clone();
            let new_charts: Vec<Chart> = old_charts
                .iter()
                .map(|c| c.renamed(&old, new_name))
                .collect();
            if new_charts != old_charts {
                let m = MetaChange::Charts {
                    sheet: si,
                    old: old_charts,
                    new: new_charts,
                };
                let _ = self.apply_meta(&m, false);
                meta.push(m);
            }
        }
        self.commit("Rename sheet", edits, meta);
        Ok(())
    }

    pub fn insert_rows(&mut self, sheet: usize, at: u32, count: u32) -> Result<(), String> {
        self.structural(sheet, Axis::Rows, at, count as i64)
    }

    pub fn delete_rows(&mut self, sheet: usize, at: u32, count: u32) -> Result<(), String> {
        self.structural(sheet, Axis::Rows, at, -(count as i64))
    }

    pub fn insert_cols(&mut self, sheet: usize, at: u32, count: u32) -> Result<(), String> {
        self.structural(sheet, Axis::Cols, at, count as i64)
    }

    pub fn delete_cols(&mut self, sheet: usize, at: u32, count: u32) -> Result<(), String> {
        self.structural(sheet, Axis::Cols, at, -(count as i64))
    }

    /// Insert (`count > 0`) or delete (`count < 0`) rows or columns at `at`:
    /// move the cells after them, adjust references in every sheet's formulas,
    /// and shift column widths, all as one undoable action.
    fn structural(&mut self, sheet: usize, axis: Axis, at: u32, count: i64) -> Result<(), String> {
        if count == 0 {
            return Ok(());
        }
        let limit = match axis {
            Axis::Rows => MAX_ROWS,
            Axis::Cols => MAX_COLS,
        };
        let index = |p: Pos| match axis {
            Axis::Rows => p.0,
            Axis::Cols => p.1,
        };
        let with_index = |p: Pos, i: u32| match axis {
            Axis::Rows => (i, p.1),
            Axis::Cols => (p.0, i),
        };
        let name = self.sheets[sheet].name.clone();
        let change = Structural {
            sheet: &name,
            axis,
            at,
            count,
        };
        let target = &self.sheets[sheet];
        if count > 0 {
            let last = target.cells.keys().map(|&p| index(p)).max().unwrap_or(0);
            if !target.cells.is_empty() && last as i64 + count >= limit as i64 {
                return Err(
                    "There isn't room: cells would be pushed off the end of the sheet".into(),
                );
            }
        }

        // Every edit, keyed by (sheet, position); later inserts win.
        let mut edits: HashMap<(usize, Pos), Option<Cell>> = HashMap::new();
        let adjust = |cell: &Cell, own: &str| -> Cell {
            if cell.is_formula() {
                Cell {
                    raw: refs::adjust_for_structure(&cell.raw, own, &change),
                    ..cell.clone()
                }
            } else {
                cell.clone()
            }
        };
        // Cells on the changed sheet that move or disappear.
        for (&pos, cell) in &target.cells {
            let i = index(pos);
            if i < at {
                continue;
            }
            edits.entry((sheet, pos)).or_insert(None);
            if let Some(ni) = change.map_index(i) {
                edits.insert((sheet, with_index(pos, ni)), Some(adjust(cell, &name)));
            }
        }
        // Formulas anywhere whose references change.
        for (si, s) in self.sheets.iter().enumerate() {
            for &pos in &s.formulas {
                if si == sheet && index(pos) >= at {
                    continue; // already moved above
                }
                let cell = &s.cells[&pos];
                let new = adjust(cell, &s.name);
                if new.raw != cell.raw {
                    edits.insert((si, pos), Some(new));
                }
            }
        }

        let mut meta = Vec::new();
        if axis == Axis::Cols {
            let old = self.sheets[sheet].col_widths.clone();
            let new: HashMap<u32, f32> = old
                .iter()
                .filter_map(|(&c, &w)| change.map_index(c).map(|nc| (nc, w)))
                .collect();
            if new != old {
                let m = MetaChange::ColWidths { sheet, old, new };
                self.apply_meta(&m, false)?;
                meta.push(m);
            }
        }

        let old_merges = self.sheets[sheet].merges.clone();
        let new_merges: Vec<Merge> = old_merges
            .iter()
            .filter_map(|&((r0, c0), (r1, c1))| {
                let ((r0, r1), (c0, c1)) = match axis {
                    Axis::Rows => (change.map_span(r0, r1)?, (c0, c1)),
                    Axis::Cols => ((r0, r1), change.map_span(c0, c1)?),
                };
                // A merge shrunk to one cell is no merge at all.
                ((r0, c0) != (r1, c1)).then_some(((r0, c0), (r1, c1)))
            })
            .collect();
        if new_merges != old_merges {
            let m = MetaChange::Merges {
                sheet,
                old: old_merges,
                new: new_merges,
            };
            self.apply_meta(&m, false)?;
            meta.push(m);
        }

        // Charts anywhere may plot data on this sheet; their anchors on it move too.
        for si in 0..self.sheets.len() {
            let old = self.sheets[si].charts.clone();
            let new: Vec<Chart> = old
                .iter()
                .filter_map(|c| c.adjusted(&change, si, sheet))
                .collect();
            if new != old {
                let m = MetaChange::Charts {
                    sheet: si,
                    old,
                    new,
                };
                self.apply_meta(&m, false)?;
                meta.push(m);
            }
        }

        // The freeze line moves with rows/columns inserted or deleted above/left of it.
        let old_freeze = self.sheets[sheet].freeze;
        let line = |n: u32| -> u32 {
            if at >= n {
                n
            } else if count > 0 {
                n + count as u32
            } else {
                n - (n.min(at + count.unsigned_abs() as u32) - at)
            }
        };
        let new_freeze = match axis {
            Axis::Rows => Freeze {
                rows: line(old_freeze.rows),
                ..old_freeze
            },
            Axis::Cols => Freeze {
                cols: line(old_freeze.cols),
                ..old_freeze
            },
        };
        if new_freeze != old_freeze {
            let m = MetaChange::Freeze {
                sheet,
                old: old_freeze,
                new: new_freeze,
            };
            self.apply_meta(&m, false)?;
            meta.push(m);
        }

        let what = match axis {
            Axis::Rows => "row",
            Axis::Cols => "column",
        };
        let n = count.unsigned_abs();
        let label = format!(
            "{} {n} {what}{}",
            if count > 0 { "Insert" } else { "Delete" },
            if n == 1 { "" } else { "s" }
        );
        let edits = edits.into_iter().map(|((s, p), c)| (s, p, c)).collect();
        self.commit(&label, edits, meta);
        Ok(())
    }

    /// Merge `range` into one cell. Only the top-left value is kept (as in
    /// Excel); merges overlapping the range are replaced.
    /// Returns how many values were dropped.
    pub fn merge(&mut self, sheet: usize, range: Merge, center: bool) -> usize {
        let ((r0, c0), (r1, c1)) = range;
        if (r0, c0) == (r1, c1) {
            return 0;
        }
        let old = self.sheets[sheet].merges.clone();
        let mut new: Vec<Merge> = old
            .iter()
            .copied()
            .filter(|&m| !merges_overlap(m, range))
            .collect();
        new.push(range);
        let meta = MetaChange::Merges { sheet, old, new };
        let _ = self.apply_meta(&meta, false);

        let s = &self.sheets[sheet];
        let mut edits: Vec<(usize, Pos, Option<Cell>)> = s
            .cells
            .iter()
            .filter(|(p, c)| merge_contains(range, **p) && **p != (r0, c0) && !c.raw.is_empty())
            .map(|(p, c)| (sheet, *p, c.formatting_only()))
            .collect();
        let dropped = edits.len();
        if center {
            let mut tl = s.cells.get(&(r0, c0)).cloned().unwrap_or_default();
            tl.style.align = HAlign::Center;
            edits.push((sheet, (r0, c0), Some(tl)));
        }
        self.commit(
            if center {
                "Merge & center"
            } else {
                "Merge cells"
            },
            edits,
            vec![meta],
        );
        dropped
    }

    /// Unmerge every merged range that overlaps `range`.
    pub fn unmerge(&mut self, sheet: usize, range: Merge) {
        let old = self.sheets[sheet].merges.clone();
        let new: Vec<Merge> = old
            .iter()
            .copied()
            .filter(|&m| !merges_overlap(m, range))
            .collect();
        if new.len() == old.len() {
            return;
        }
        let meta = MetaChange::Merges { sheet, old, new };
        let _ = self.apply_meta(&meta, false);
        self.commit("Unmerge cells", Vec::new(), vec![meta]);
    }

    /// Freeze the given number of top rows and left columns (0, 0 unfreezes).
    pub fn set_freeze(&mut self, sheet: usize, freeze: Freeze) {
        let old = self.sheets[sheet].freeze;
        if old == freeze {
            return;
        }
        let meta = MetaChange::Freeze {
            sheet,
            old,
            new: freeze,
        };
        let _ = self.apply_meta(&meta, false);
        let label = if freeze.is_active() {
            "Freeze panes"
        } else {
            "Unfreeze panes"
        };
        self.commit(label, Vec::new(), vec![meta]);
    }

    /// Replace a sheet's charts (add, move, resize, edit or delete) as one undoable step.
    pub fn set_charts(&mut self, sheet: usize, charts: Vec<Chart>, label: &str) {
        let old = self.sheets[sheet].charts.clone();
        if old == charts {
            return;
        }
        let meta = MetaChange::Charts {
            sheet,
            old,
            new: charts,
        };
        let _ = self.apply_meta(&meta, false);
        self.commit(label, Vec::new(), vec![meta]);
    }

    pub fn delete_sheet(&mut self, idx: usize) {
        if self.sheets.len() <= 1 {
            return;
        }
        let name = self.sheets.remove(idx).name;
        let _ = self.engine.delete_sheet(&name);
        // History refers to sheets by index, which just shifted.
        self.undo.clear();
        self.redo.clear();
        self.dirty = true;
        self.recalc();
    }

    /// Grow the engine's column storage to cover `max` in one step. Without
    /// this, writing row by row regrows every column per new row (O(n²)).
    fn reserve(&mut self, sheet: usize, max: Pos) {
        let s = &self.sheets[sheet];
        if max.0 < s.n_rows && max.1 < s.n_cols {
            return;
        }
        let (r, c) = (max.0.max(s.n_rows) + 1, max.1.max(s.n_cols) + 1);
        if !s.cells.contains_key(&(r - 1, c - 1)) {
            let name = s.name.clone();
            let _ = self.engine.set_value(&name, r, c, LiteralValue::Empty);
        }
    }

    /// Apply a batch of edits as one undoable action.
    pub fn apply(&mut self, label: &str, edits: Vec<(usize, Pos, Option<Cell>)>) {
        self.commit(label, edits, Vec::new());
    }

    /// Write `edits` and record them, with `meta` changes that have already
    /// been applied, as one undoable action.
    fn commit(
        &mut self,
        label: &str,
        edits: Vec<(usize, Pos, Option<Cell>)>,
        meta: Vec<MetaChange>,
    ) {
        let mut extents: HashMap<usize, Pos> = HashMap::new();
        for (sheet, pos, cell) in &edits {
            if cell.is_some() {
                let e = extents.entry(*sheet).or_insert(*pos);
                *e = (e.0.max(pos.0), e.1.max(pos.1));
            }
        }
        for (sheet, max) in extents {
            self.reserve(sheet, max);
        }
        let mut changes = Vec::with_capacity(edits.len());
        let mut touched = HashSet::new();
        for (sheet, pos, new) in edits {
            let new = new.filter(|c| !c.is_blank());
            let old = self.sheets[sheet].cells.get(&pos).cloned();
            if old == new {
                continue;
            }
            self.write_cell(sheet, pos, new.clone());
            touched.insert(sheet);
            changes.push(CellChange {
                sheet,
                pos,
                old,
                new,
            });
        }
        if changes.is_empty() && meta.is_empty() {
            return;
        }
        for s in touched {
            self.sheets[s].recompute_extent();
        }
        self.recalc();
        self.dirty = true;
        self.redo.clear();
        self.undo.push(HistoryEntry {
            label: label.to_string(),
            meta,
            changes,
        });
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    /// Set a cell from user-typed text (interprets dates and percentages).
    pub fn set_input(&mut self, sheet: usize, pos: Pos, input: &str) {
        let existing = self.sheets[sheet].cells.get(&pos);
        let style = existing.map(|c| c.style).unwrap_or_default();
        let cell = interpret_input(input, self.sheets[sheet].fmt(pos))
            .map(|c| Cell { style, ..c })
            .or_else(|| existing.and_then(Cell::formatting_only));
        self.apply("Edit cell", vec![(sheet, pos, cell)]);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Returns the cells touched, so the UI can move the selection there.
    pub fn undo(&mut self) -> Option<(usize, Vec<Pos>)> {
        let entry = self.undo.pop()?;
        let r = self.replay(&entry, true);
        self.redo.push(entry);
        Some(r)
    }

    pub fn redo(&mut self) -> Option<(usize, Vec<Pos>)> {
        let entry = self.redo.pop()?;
        let r = self.replay(&entry, false);
        self.undo.push(entry);
        Some(r)
    }

    fn replay(&mut self, entry: &HistoryEntry, backwards: bool) -> (usize, Vec<Pos>) {
        if !backwards {
            for m in &entry.meta {
                let _ = self.apply_meta(m, false);
            }
        }
        let mut touched = HashSet::new();
        let iter: Box<dyn Iterator<Item = &CellChange>> = if backwards {
            Box::new(entry.changes.iter().rev())
        } else {
            Box::new(entry.changes.iter())
        };
        for ch in iter {
            let target = if backwards {
                ch.old.clone()
            } else {
                ch.new.clone()
            };
            self.write_cell(ch.sheet, ch.pos, target);
            touched.insert(ch.sheet);
        }
        if backwards {
            for m in entry.meta.iter().rev() {
                let _ = self.apply_meta(m, true);
            }
        }
        for s in touched {
            self.sheets[s].recompute_extent();
        }
        self.recalc();
        self.dirty = true;
        let sheet = entry
            .changes
            .first()
            .map(|c| c.sheet)
            .or_else(|| entry.meta.first().map(MetaChange::sheet))
            .unwrap_or(0);
        (sheet, entry.changes.iter().map(|c| c.pos).collect())
    }

    /// Apply (or, with `backwards`, revert) a non-cell change.
    fn apply_meta(&mut self, change: &MetaChange, backwards: bool) -> Result<(), String> {
        match change {
            MetaChange::RenameSheet { sheet, old, new } => {
                let (from, to) = if backwards { (new, old) } else { (old, new) };
                self.engine
                    .rename_sheet(from, to)
                    .map_err(|e| e.to_string())?;
                self.sheets[*sheet].name = to.clone();
            }
            MetaChange::ColWidths { sheet, old, new } => {
                self.sheets[*sheet].col_widths = if backwards { old } else { new }.clone();
            }
            MetaChange::Merges { sheet, old, new } => {
                self.sheets[*sheet].merges = if backwards { old } else { new }.clone();
            }
            MetaChange::Freeze { sheet, old, new } => {
                self.sheets[*sheet].freeze = if backwards { *old } else { *new };
            }
            MetaChange::Charts { sheet, old, new } => {
                self.sheets[*sheet].charts = if backwards { old } else { new }.clone();
            }
        }
        self.dirty = true;
        Ok(())
    }

    /// Write one cell into both our model and the engine. Does not recalc.
    fn write_cell(&mut self, sheet: usize, pos: Pos, cell: Option<Cell>) {
        let (r, c) = (pos.0 + 1, pos.1 + 1);
        let Self { sheets, engine, .. } = self;
        let s = &mut sheets[sheet];
        s.formulas.remove(&pos);
        match cell {
            None => {
                s.cells.remove(&pos);
                s.values.remove(&pos);
                let _ = engine.set_value(&s.name, r, c, LiteralValue::Empty);
            }
            Some(cell) => {
                if cell.raw.is_empty() {
                    // Formatted but empty.
                    s.values.remove(&pos);
                    let _ = engine.set_value(&s.name, r, c, LiteralValue::Empty);
                    s.cells.insert(pos, cell);
                    return;
                }
                if cell.is_formula() {
                    s.formulas.insert(pos);
                    s.values.insert(pos, Value::Empty);
                    if engine.set_formula(&s.name, r, c, &cell.raw).is_err() {
                        s.values.insert(pos, Value::Error("#NAME?".into()));
                    }
                } else {
                    let v = if cell.fmt == NumFmt::Text {
                        Value::Text(cell.raw.clone())
                    } else {
                        parse_literal(&cell.raw)
                    };
                    let _ = engine.set_value(&s.name, r, c, to_literal(&v));
                    s.values.insert(pos, v);
                }
                s.n_rows = s.n_rows.max(pos.0 + 1);
                s.n_cols = s.n_cols.max(pos.1 + 1);
                s.cells.insert(pos, cell);
            }
        }
    }

    /// Recalculate and pull formula results back into the model.
    pub fn recalc(&mut self) {
        let _ = self.engine.evaluate_all();
        for s in &mut self.sheets {
            for &pos in &s.formulas {
                let v = match self.engine.get_value(&s.name, pos.0 + 1, pos.1 + 1) {
                    Some(lv) => from_literal(&lv),
                    None => Value::Empty,
                };
                // Keep our parse-error marker if the engine has nothing better.
                if v == Value::Empty
                    && matches!(s.values.get(&pos), Some(Value::Error(e)) if e == "#NAME?")
                {
                    continue;
                }
                s.values.insert(pos, v);
            }
        }
    }
}

fn to_literal(v: &Value) -> LiteralValue {
    match v {
        Value::Empty => LiteralValue::Empty,
        Value::Number(n) => LiteralValue::Number(*n),
        Value::Text(s) => LiteralValue::Text(s.clone()),
        Value::Bool(b) => LiteralValue::Boolean(*b),
        Value::Error(_) => LiteralValue::Text(String::new()),
    }
}

fn from_literal(lv: &LiteralValue) -> Value {
    match lv {
        LiteralValue::Int(i) => Value::Number(*i as f64),
        LiteralValue::Number(n) => Value::Number(*n),
        LiteralValue::Text(s) => Value::Text(s.clone()),
        LiteralValue::Boolean(b) => Value::Bool(*b),
        LiteralValue::Date(d) => Value::Number(date_to_serial(d.and_hms_opt(0, 0, 0).unwrap())),
        LiteralValue::DateTime(dt) => Value::Number(date_to_serial(*dt)),
        LiteralValue::Time(t) => Value::Number(t.num_seconds_from_midnight() as f64 / 86400.0),
        LiteralValue::Duration(d) => Value::Number(d.num_seconds() as f64 / 86400.0),
        LiteralValue::Array(rows) => rows
            .first()
            .and_then(|r| r.first())
            .map(from_literal)
            .unwrap_or(Value::Empty),
        LiteralValue::Empty => Value::Number(0.0),
        LiteralValue::Pending => Value::Empty,
        LiteralValue::Error(e) => Value::Error(e.kind.to_string()),
    }
}

impl MetaChange {
    /// The sheet this change belongs to.
    pub fn sheet(&self) -> usize {
        match self {
            MetaChange::RenameSheet { sheet, .. }
            | MetaChange::ColWidths { sheet, .. }
            | MetaChange::Merges { sheet, .. }
            | MetaChange::Freeze { sheet, .. }
            | MetaChange::Charts { sheet, .. } => *sheet,
        }
    }
}
