//! A single worksheet: cells, computed values and column widths.

use std::collections::{HashMap, HashSet};

use super::{Cell, NumFmt, Pos, Value, format_value, refs};

/// A merged range: (top-left, bottom-right), inclusive.
pub type Merge = (Pos, Pos);

pub fn merge_contains(m: Merge, p: Pos) -> bool {
    (m.0.0..=m.1.0).contains(&p.0) && (m.0.1..=m.1.1).contains(&p.1)
}

pub fn merges_overlap(a: Merge, b: Merge) -> bool {
    a.0.0 <= b.1.0 && b.0.0 <= a.1.0 && a.0.1 <= b.1.1 && b.0.1 <= a.1.1
}

/// Width of a column that hasn't been resized, in points.
pub const DEFAULT_COL_WIDTH: f32 = 100.0;

pub struct Sheet {
    pub name: String,
    pub cells: HashMap<Pos, Cell>,
    /// Computed value for every non-empty cell.
    pub values: HashMap<Pos, Value>,
    pub(crate) formulas: HashSet<Pos>,
    pub col_widths: HashMap<u32, f32>,
    /// Merged ranges as (top-left, bottom-right), inclusive. Never overlapping.
    pub merges: Vec<Merge>,
    /// Used extent: one past the last non-empty row/col.
    pub n_rows: u32,
    pub n_cols: u32,
}

impl Sheet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cells: HashMap::new(),
            values: HashMap::new(),
            formulas: HashSet::new(),
            col_widths: HashMap::new(),
            merges: Vec::new(),
            n_rows: 0,
            n_cols: 0,
        }
    }

    #[cfg(test)]
    pub fn raw(&self, pos: Pos) -> &str {
        self.cells.get(&pos).map(|c| c.raw.as_str()).unwrap_or("")
    }

    pub fn value(&self, pos: Pos) -> &Value {
        self.values.get(&pos).unwrap_or(&Value::Empty)
    }

    pub fn fmt(&self, pos: Pos) -> NumFmt {
        self.cells.get(&pos).map(|c| c.fmt).unwrap_or_default()
    }

    /// The value as shown in the grid.
    pub fn display(&self, pos: Pos) -> String {
        format_value(self.value(pos), self.fmt(pos))
    }

    /// The text to put in the editor when the user starts editing.
    pub fn edit_text(&self, pos: Pos) -> String {
        match self.cells.get(&pos) {
            None => String::new(),
            Some(c) if c.is_formula() => c.raw.clone(),
            Some(c) => match (c.fmt, self.value(pos)) {
                (
                    NumFmt::Date | NumFmt::DateTime | NumFmt::Time | NumFmt::Percent { .. },
                    v @ Value::Number(_),
                ) => format_value(v, c.fmt),
                _ => c.raw.clone(),
            },
        }
    }

    /// The merged range containing `pos`, if any.
    pub fn merge_at(&self, pos: Pos) -> Option<Merge> {
        self.merges
            .iter()
            .copied()
            .find(|&m| merge_contains(m, pos))
    }

    /// The used range counts cells with content, not ones that only carry a format.
    pub(crate) fn recompute_extent(&mut self) {
        let used = || {
            self.cells
                .iter()
                .filter(|(_, c)| !c.raw.is_empty())
                .map(|(p, _)| *p)
        };
        self.n_rows = used().map(|p| p.0 + 1).max().unwrap_or(0);
        self.n_cols = used().map(|p| p.1 + 1).max().unwrap_or(0);
    }
}

/// Rows `rows` reordered so that row contents move with relative references
/// adjusted (the way Excel moves rows during a sort).
pub fn move_rows_edits(
    sheet: &Sheet,
    sheet_idx: usize,
    rows: &[u32],
    new_order: &[u32],
    cols: std::ops::Range<u32>,
) -> Vec<(usize, Pos, Option<Cell>)> {
    let mut edits = Vec::new();
    for (dest, src) in rows.iter().zip(new_order) {
        for c in cols.clone() {
            let cell = sheet.cells.get(&(*src, c)).map(|cell| {
                let mut cell = cell.clone();
                if cell.is_formula() {
                    cell.raw = refs::shift_formula(&cell.raw, *dest as i64 - *src as i64, 0);
                }
                cell
            });
            edits.push((sheet_idx, (*dest, c), cell));
        }
    }
    edits
}
