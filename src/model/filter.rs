//! Per-sheet view state: column filters, global search, and the resulting
//! list of visible rows.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::refs;
use super::{Pos, Sheet, Value, compare_values, parse_date};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Eq,
    Neq,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Gt,
    Gte,
    Lt,
    Lte,
    Empty,
    NotEmpty,
}

impl Op {
    pub const ALL: [Op; 12] = [
        Op::Eq,
        Op::Neq,
        Op::Contains,
        Op::NotContains,
        Op::StartsWith,
        Op::EndsWith,
        Op::Gt,
        Op::Gte,
        Op::Lt,
        Op::Lte,
        Op::Empty,
        Op::NotEmpty,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Op::Eq => "equals",
            Op::Neq => "does not equal",
            Op::Contains => "contains",
            Op::NotContains => "does not contain",
            Op::StartsWith => "starts with",
            Op::EndsWith => "ends with",
            Op::Gt => ">",
            Op::Gte => "≥",
            Op::Lt => "<",
            Op::Lte => "≤",
            Op::Empty => "is empty",
            Op::NotEmpty => "is not empty",
        }
    }

    pub fn needs_value(self) -> bool {
        !matches!(self, Op::Empty | Op::NotEmpty)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub op: Op,
    #[serde(default)]
    pub value: String,
}

impl Condition {
    pub fn matches(&self, value: &Value, display: &str) -> bool {
        let text = display.to_lowercase();
        let want = self.value.trim().to_lowercase();
        let want_num = want
            .replace(',', "")
            .parse::<f64>()
            .ok()
            .or_else(|| parse_date(&want));
        let num = value.as_number();
        let cmp = || -> Option<std::cmp::Ordering> {
            match (num, want_num) {
                (Some(a), Some(b)) => a.partial_cmp(&b),
                (None, Some(_)) => None,
                _ => Some(text.as_str().cmp(want.as_str())),
            }
        };
        match self.op {
            Op::Eq => match (num, want_num) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-9,
                _ => text == want,
            },
            Op::Neq => !Condition {
                op: Op::Eq,
                value: self.value.clone(),
            }
            .matches(value, display),
            Op::Contains => text.contains(&want),
            Op::NotContains => !text.contains(&want),
            Op::StartsWith => text.starts_with(&want),
            Op::EndsWith => text.ends_with(&want),
            Op::Gt => cmp().is_some_and(|o| o.is_gt()),
            Op::Gte => cmp().is_some_and(|o| o.is_ge()),
            Op::Lt => cmp().is_some_and(|o| o.is_lt()),
            Op::Lte => cmp().is_some_and(|o| o.is_le()),
            Op::Empty => display.trim().is_empty(),
            Op::NotEmpty => !display.trim().is_empty(),
        }
    }

    pub fn describe(&self) -> String {
        if self.op.needs_value() {
            format!("{} {}", self.op.label(), self.value)
        } else {
            self.op.label().to_string()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnFilter {
    /// All must match.
    pub conditions: Vec<Condition>,
    /// If set, only these display values pass.
    pub allowed: Option<BTreeSet<String>>,
}

impl ColumnFilter {
    pub fn is_active(&self) -> bool {
        !self.conditions.is_empty() || self.allowed.is_some()
    }
    fn matches(&self, value: &Value, display: &str) -> bool {
        if let Some(allowed) = &self.allowed
            && !allowed.contains(display)
        {
            return false;
        }
        self.conditions.iter().all(|c| c.matches(value, display))
    }
}

pub struct ViewState {
    pub filters: BTreeMap<u32, ColumnFilter>,
    /// Keep the first row visible (as a header) when filtering.
    pub header_row: bool,
    pub search: String,
    pub search_only_matching: bool,
    /// Visible data rows, or `None` when every row is visible.
    pub rows: Option<Vec<u32>>,
    pub matches: Vec<Pos>,
    pub current_match: usize,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            filters: BTreeMap::new(),
            header_row: true,
            search: String::new(),
            search_only_matching: false,
            rows: None,
            matches: Vec::new(),
            current_match: 0,
        }
    }
}

impl ViewState {
    pub fn has_filters(&self) -> bool {
        self.filters.values().any(|f| f.is_active())
    }

    pub fn is_filtered(&self) -> bool {
        self.rows.is_some()
    }

    /// Recompute visible rows and search matches after data or filter changes.
    pub fn refresh(&mut self, sheet: &Sheet) {
        self.filters.retain(|_, f| f.is_active());
        let query = self.search.trim().to_lowercase();

        self.matches.clear();
        if !query.is_empty() {
            // Numbers and dates only ever display digits and . - % : E, so
            // skip formatting them when the query can't match.
            let numeric_query = query
                .chars()
                .all(|c| c.is_ascii_digit() || ".-%: e+".contains(c));
            for (&pos, value) in sheet.values.iter() {
                let hit = match value {
                    Value::Text(s) | Value::Error(s) => contains_ci(s, &query),
                    Value::Number(_) if !numeric_query => false,
                    Value::Empty => false,
                    _ => sheet.display(pos).to_lowercase().contains(&query),
                };
                if hit {
                    self.matches.push(pos);
                }
            }
            self.matches.sort_unstable();
        }
        if self.current_match >= self.matches.len() {
            self.current_match = 0;
        }

        let search_filters = self.search_only_matching && !query.is_empty();
        if !self.has_filters() && !search_filters {
            self.rows = None;
            return;
        }
        let match_rows: BTreeSet<u32> = self.matches.iter().map(|p| p.0).collect();
        let first = if self.header_row { 1 } else { 0 };
        let mut rows: Vec<u32> = (0..first.min(sheet.n_rows)).collect();
        for r in first..sheet.n_rows {
            if search_filters && !match_rows.contains(&r) {
                continue;
            }
            let ok = self.filters.iter().all(|(&c, f)| {
                let pos = (r, c);
                f.matches(sheet.value(pos), &sheet.display(pos))
            });
            if ok {
                rows.push(r);
            }
        }
        self.rows = Some(rows);
    }

    /// Distinct display values in a column (for the filter checklist), capped.
    pub fn distinct_values(&self, sheet: &Sheet, col: u32, cap: usize) -> Vec<String> {
        let first = if self.header_row { 1 } else { 0 };
        let mut seen = std::collections::HashSet::new();
        let mut values = Vec::new();
        for r in first..sheet.n_rows {
            let display = sheet.display((r, col));
            if seen.insert(display.clone()) {
                values.push((sheet.value((r, col)).clone(), display));
                if values.len() >= cap {
                    break;
                }
            }
        }
        // Same order as sorting the column: numbers numerically, then text, blanks last.
        values.sort_by(|a, b| match (&a.0, &b.0) {
            (Value::Empty, Value::Empty) => std::cmp::Ordering::Equal,
            (Value::Empty, _) => std::cmp::Ordering::Greater,
            (_, Value::Empty) => std::cmp::Ordering::Less,
            (x, y) => compare_values(x, y),
        });
        values.into_iter().map(|v| v.1).collect()
    }

    pub fn describe_filters(&self, sheet: &Sheet) -> Vec<String> {
        self.filters
            .iter()
            .filter(|(_, f)| f.is_active())
            .map(|(&c, f)| {
                let mut parts: Vec<String> = f.conditions.iter().map(|c| c.describe()).collect();
                if let Some(a) = &f.allowed {
                    parts.push(format!("one of {} values", a.len()));
                }
                format!(
                    "{}: {}",
                    column_label(sheet, c, self.header_row),
                    parts.join(" and ")
                )
            })
            .collect()
    }
}

/// Case-insensitive substring test; `needle` must already be lowercase.
fn contains_ci(hay: &str, needle: &str) -> bool {
    if hay.is_ascii() && needle.is_ascii() {
        let (h, n) = (hay.as_bytes(), needle.as_bytes());
        n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
    } else {
        hay.to_lowercase().contains(needle)
    }
}

/// "B (Price)" if the first row looks like a header, else "B".
pub fn column_label(sheet: &Sheet, col: u32, header_row: bool) -> String {
    let header = sheet.display((0, col));
    if header_row && !header.is_empty() {
        format!("{} ({})", refs::col_name(col), header)
    } else {
        refs::col_name(col)
    }
}

/// Resolve a column given as a letter ("C") or header text ("Price").
pub fn resolve_column(sheet: &Sheet, name: &str) -> Option<u32> {
    let name = name.trim();
    for c in 0..sheet.n_cols {
        if sheet.display((0, c)).trim().eq_ignore_ascii_case(name) {
            return Some(c);
        }
    }
    let letters = name.trim_start_matches('$');
    if letters.len() <= 3 && letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return refs::col_index(letters);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Book;

    #[test]
    fn filters_and_search() {
        let mut b = Book::new_blank();
        let data = [
            ["Name", "Qty"],
            ["apple", "5"],
            ["banana", "12"],
            ["cherry", "40"],
            ["apricot", "8"],
        ];
        for (r, row) in data.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                b.set_input(0, (r as u32, c as u32), v);
            }
        }
        let s = &b.sheets[0];
        let mut v = ViewState::default();
        v.filters.insert(
            1,
            ColumnFilter {
                conditions: vec![Condition {
                    op: Op::Gte,
                    value: "8".into(),
                }],
                allowed: None,
            },
        );
        v.refresh(s);
        assert_eq!(v.rows.as_deref(), Some(&[0, 2, 3, 4][..]));

        v.filters.clear();
        v.search = "AP".into();
        v.search_only_matching = true;
        v.refresh(s);
        assert_eq!(v.rows.as_deref(), Some(&[0, 1, 4][..]));
        assert_eq!(v.matches, vec![(1, 0), (4, 0)]);

        assert_eq!(resolve_column(s, "qty"), Some(1));
        assert_eq!(v.distinct_values(s, 1, 100), ["5", "8", "12", "40"]);
        assert_eq!(resolve_column(s, "A"), Some(0));
    }
}
