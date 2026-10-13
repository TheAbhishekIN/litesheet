//! Cells: raw user input, number formats, and interpreting typed text.

use super::format::{fmt_number, parse_date};

/// Zero-based (row, col).
pub type Pos = (u32, u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumFmt {
    #[default]
    General,
    Date,
    DateTime,
    Percent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// What the user typed: `=SUM(A1:A3)`, `42`, `hello`, `'0042`.
    pub raw: String,
    pub fmt: NumFmt,
}

impl Cell {
    pub fn new(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            fmt: NumFmt::General,
        }
    }
    pub fn is_formula(&self) -> bool {
        is_formula(&self.raw)
    }
}

pub fn is_formula(raw: &str) -> bool {
    raw.len() > 1 && raw.starts_with('=')
}

/// Turn user input into a cell, recognising dates (2024-05-01) and percentages (15%).
pub fn interpret_input(input: &str, prev_fmt: NumFmt) -> Option<Cell> {
    let t = input.trim();
    if input.is_empty() {
        return None;
    }
    if is_formula(t) || t.starts_with('\'') {
        return Some(Cell::new(input));
    }
    if let Some(serial) = parse_date(t) {
        let fmt = if serial.fract() != 0.0 {
            NumFmt::DateTime
        } else {
            NumFmt::Date
        };
        return Some(Cell {
            raw: fmt_number(serial),
            fmt,
        });
    }
    if let Some(p) = t.strip_suffix('%')
        && let Ok(n) = p.trim().parse::<f64>()
    {
        return Some(Cell {
            raw: fmt_number(n / 100.0),
            fmt: NumFmt::Percent,
        });
    }
    let fmt = if t.parse::<f64>().is_ok() {
        prev_fmt
    } else {
        NumFmt::General
    };
    Some(Cell {
        raw: input.to_string(),
        fmt,
    })
}
