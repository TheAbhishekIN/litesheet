//! Excel import (via calamine: .xlsx, .xlsm, .xls, .ods) and .xlsx export
//! (via rust_xlsxwriter).

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use calamine::{Data, Reader, open_workbook_auto};
use rust_xlsxwriter::{Format, Formula, Workbook as XlsxWorkbook};

use crate::model::{Book, Cell, NumFmt, Pos, Value, fmt_number, parse_literal};

/// Rough conversion between Excel character widths and our point widths.
const PX_PER_CHAR: f32 = 7.5;

pub(super) fn load(path: &Path) -> Result<Book> {
    let mut wb =
        open_workbook_auto(path).with_context(|| format!("Couldn't open {}", path.display()))?;
    let mut sheets = Vec::new();
    for name in wb.sheet_names() {
        let mut cells: std::collections::HashMap<Pos, Cell> = std::collections::HashMap::new();
        if let Ok(range) = wb.worksheet_range(&name) {
            let (r0, c0) = range.start().unwrap_or((0, 0));
            for (r, c, data) in range.used_cells() {
                if let Some(cell) = cell_from_data(data) {
                    cells.insert((r0 + r as u32, c0 + c as u32), cell);
                }
            }
        }
        if let Ok(formulas) = wb.worksheet_formula(&name) {
            let (r0, c0) = formulas.start().unwrap_or((0, 0));
            for (r, c, f) in formulas.used_cells() {
                if f.is_empty() {
                    continue;
                }
                let raw = if f.starts_with('=') {
                    f.clone()
                } else {
                    format!("={f}")
                };
                let pos = (r0 + r as u32, c0 + c as u32);
                let fmt = cells.get(&pos).map(|c| c.fmt).unwrap_or_default();
                cells.insert(pos, Cell { raw, fmt });
            }
        }
        sheets.push((name, cells.into_iter().collect()));
    }
    if sheets.is_empty() {
        return Err(anyhow!("The workbook has no sheets"));
    }
    Ok(Book::from_sheets(sheets))
}

fn cell_from_data(d: &Data) -> Option<Cell> {
    Some(match d {
        Data::Empty => return None,
        Data::Int(i) => Cell::new(i.to_string()),
        Data::Float(f) => Cell::new(fmt_number(*f)),
        Data::Bool(b) => Cell::new(if *b { "TRUE" } else { "FALSE" }),
        Data::String(s) => {
            // Text that would otherwise be re-read as a number/formula keeps a quote prefix.
            if matches!(parse_literal(s), Value::Text(_))
                && !s.starts_with('=')
                && !s.starts_with('\'')
            {
                Cell::new(s.clone())
            } else {
                Cell::new(format!("'{s}"))
            }
        }
        Data::DateTime(dt) => {
            let serial = dt.as_f64();
            let fmt = if serial.fract() != 0.0 && serial >= 1.0 {
                NumFmt::DateTime
            } else {
                NumFmt::Date
            };
            Cell {
                raw: fmt_number(serial),
                fmt,
            }
        }
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::new(s.clone()),
        Data::Error(e) => Cell::new(format!("'{e}")),
    })
}

pub(super) fn save(book: &Book, path: &Path) -> Result<()> {
    let mut wb = XlsxWorkbook::new();
    let date_fmt = Format::new().set_num_format("yyyy-mm-dd");
    let datetime_fmt = Format::new().set_num_format("yyyy-mm-dd hh:mm");
    let pct_fmt = Format::new().set_num_format("0.00%");
    for sheet in &book.sheets {
        let ws = wb.add_worksheet();
        ws.set_name(&sheet.name)?;
        for (&col, &w) in &sheet.col_widths {
            if col <= u16::MAX as u32 {
                ws.set_column_width(col as u16, (w / PX_PER_CHAR) as f64)?;
            }
        }
        let mut positions: Vec<&Pos> = sheet.cells.keys().collect();
        positions.sort_unstable();
        for &pos in positions {
            let cell = &sheet.cells[&pos];
            let (r, c) = (pos.0, pos.1 as u16);
            let format = match cell.fmt {
                NumFmt::General => None,
                NumFmt::Date => Some(&date_fmt),
                NumFmt::DateTime => Some(&datetime_fmt),
                NumFmt::Percent => Some(&pct_fmt),
            };
            if cell.is_formula() {
                let result = sheet.display(pos);
                let formula = Formula::new(&cell.raw).set_result(result);
                match format {
                    Some(f) => ws.write_formula_with_format(r, c, formula, f)?,
                    None => ws.write_formula(r, c, formula)?,
                };
                continue;
            }
            match (sheet.value(pos), format) {
                (Value::Number(n), Some(f)) => ws.write_number_with_format(r, c, *n, f)?,
                (Value::Number(n), None) => ws.write_number(r, c, *n)?,
                (Value::Bool(b), _) => ws.write_boolean(r, c, *b)?,
                (Value::Text(s), _) => ws.write_string(r, c, s)?,
                (Value::Error(e), _) => ws.write_string(r, c, e)?,
                (Value::Empty, _) => ws,
            };
        }
    }
    wb.save(path)
        .with_context(|| format!("Couldn't write {}", path.display()))?;
    Ok(())
}
