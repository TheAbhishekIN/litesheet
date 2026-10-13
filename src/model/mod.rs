//! The spreadsheet data model, independent of the UI.
//!
//! - [`Book`] owns the sheets, the undo history and the formula engine.
//! - [`Sheet`] holds raw cell input ([`Cell`]) and computed [`Value`]s.
//! - [`refs`] handles A1 references; [`filter`] computes which rows are visible.

mod book;
mod cell;
pub mod filter;
mod format;
pub mod refs;
mod sheet;
mod value;

pub use book::{Book, CellChange, MAX_COLS, MAX_ROWS, MetaChange};
pub use cell::{Cell, NumFmt, Pos, interpret_input, is_formula};
pub use format::{date_to_serial, fmt_number, format_value, parse_date, serial_to_datetime};
pub use sheet::{DEFAULT_COL_WIDTH, Sheet, move_rows_edits};
pub use value::{Value, compare_values, parse_literal};

#[cfg(test)]
mod tests;
