//! Excel import (via calamine: .xlsx, .xlsm, .xls, .ods) and .xlsx export
//! (via rust_xlsxwriter).

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use calamine::{Data, Reader, open_workbook_auto};
use rust_xlsxwriter::{
    Color, Format, FormatAlign, FormatBorder, FormatUnderline, Formula, Workbook as XlsxWorkbook,
};

use super::xlsx_styles;
use crate::model::{Book, Cell, HAlign, NumFmt, Pos, Style, Value, fmt_number, parse_literal};

/// Rough conversion between Excel character widths and our point widths.
const PX_PER_CHAR: f32 = 7.5;

pub(super) fn load(path: &Path) -> Result<Book> {
    let mut wb =
        open_workbook_auto(path).with_context(|| format!("Couldn't open {}", path.display()))?;
    // Styles, number formats and widths only exist for the OOXML formats.
    let is_ooxml = matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("xlsx" | "xlsm")
    );
    let mut formatting = if is_ooxml {
        xlsx_styles::read(path).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let mut widths = Vec::new();
    let mut sheets = Vec::new();
    for name in wb.sheet_names() {
        let mut cells: HashMap<Pos, Cell> = HashMap::new();
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
                cells.insert(pos, Cell::with_fmt(raw, fmt));
            }
        }
        if let Some(f) = formatting.remove(&name) {
            for (pos, fmt, style) in f.cells {
                let cell = cells.entry(pos).or_insert_with(|| Cell::new(""));
                cell.style = style;
                // calamine already recognised dates; keep that if the code says General.
                if fmt != NumFmt::General {
                    cell.fmt = fmt;
                }
                if cell.fmt == NumFmt::Text
                    && let Some(text) = cell.raw.strip_prefix('\'')
                {
                    cell.raw = text.to_string();
                }
            }
            widths.push((f.col_widths, f.merges, f.freeze));
        } else {
            widths.push((Vec::new(), Vec::new(), None));
        }
        sheets.push((name, cells.into_iter().collect()));
    }
    if sheets.is_empty() {
        return Err(anyhow!("The workbook has no sheets"));
    }
    let mut book = Book::from_sheets(sheets);
    for (sheet, (widths, merges, freeze)) in book.sheets.iter_mut().zip(widths) {
        for (col, chars) in widths {
            sheet
                .col_widths
                .insert(col, (chars as f32 * PX_PER_CHAR).round());
        }
        sheet.merges = merges.into_iter().filter(|(a, b)| a != b).collect();
        if let Some((rows, cols)) = freeze {
            sheet.freeze = crate::model::Freeze { rows, cols };
        }
    }
    Ok(book)
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
                style: Style::default(),
            }
        }
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::new(s.clone()),
        Data::Error(e) => Cell::new(format!("'{e}")),
    })
}

pub(super) fn save(book: &Book, path: &Path) -> Result<()> {
    let mut wb = XlsxWorkbook::new();
    let mut formats: HashMap<(NumFmt, Style), Format> = HashMap::new();
    for sheet in &book.sheets {
        let ws = wb.add_worksheet();
        ws.set_name(&sheet.name)?;
        for (&col, &w) in &sheet.col_widths {
            if col <= u16::MAX as u32 {
                ws.set_column_width(col as u16, (w / PX_PER_CHAR) as f64)?;
            }
        }
        if sheet.freeze.is_active() {
            ws.set_freeze_panes(
                sheet.freeze.rows,
                sheet.freeze.cols.min(u16::MAX as u32) as u16,
            )?;
        }
        // Merges first: merge_range writes the range as text, so the real
        // top-left value (number, formula, …) is written over it below.
        for &((r0, c0), (r1, c1)) in &sheet.merges {
            if c1 > u16::MAX as u32 {
                continue;
            }
            let tl = sheet.cells.get(&(r0, c0));
            let format = tl
                .map(|c| excel_format(c.fmt, &c.style))
                .unwrap_or_default();
            ws.merge_range(r0, c0 as u16, r1, c1 as u16, "", &format)?;
        }
        let mut positions: Vec<&Pos> = sheet.cells.keys().collect();
        positions.sort_unstable();
        for &pos in positions {
            let cell = &sheet.cells[&pos];
            let (r, c) = (pos.0, pos.1 as u16);
            let format = (cell.fmt != NumFmt::General || !cell.style.is_default()).then(|| {
                &*formats
                    .entry((cell.fmt, cell.style))
                    .or_insert_with(|| excel_format(cell.fmt, &cell.style))
            });
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
                (Value::Text(s), Some(f)) => ws.write_string_with_format(r, c, s, f)?,
                (Value::Text(s), None) => ws.write_string(r, c, s)?,
                (Value::Error(e), _) => ws.write_string(r, c, e)?,
                // A formatted empty cell keeps its format.
                (Value::Empty, Some(f)) => ws.write_blank(r, c, f)?,
                (Value::Empty, None) => ws,
            };
        }
    }
    wb.save(path)
        .with_context(|| format!("Couldn't write {}", path.display()))?;
    Ok(())
}

/// The rust_xlsxwriter format for a number format and style.
fn excel_format(fmt: NumFmt, style: &Style) -> Format {
    let mut f = Format::new();
    if let Some(code) = fmt.excel_code() {
        f = f.set_num_format(code);
    }
    if style.bold {
        f = f.set_bold();
    }
    if style.italic {
        f = f.set_italic();
    }
    if style.underline {
        f = f.set_underline(FormatUnderline::Single);
    }
    if style.strike {
        f = f.set_font_strikethrough();
    }
    if let Some(c) = style.color {
        f = f.set_font_color(Color::RGB(c.to_u32()));
    }
    if let Some(c) = style.fill {
        f = f.set_background_color(Color::RGB(c.to_u32()));
    }
    f = match style.align {
        HAlign::General => f,
        HAlign::Left => f.set_align(FormatAlign::Left),
        HAlign::Center => f.set_align(FormatAlign::Center),
        HAlign::Right => f.set_align(FormatAlign::Right),
    };
    let b = style.borders;
    if b.top {
        f = f.set_border_top(FormatBorder::Thin);
    }
    if b.bottom {
        f = f.set_border_bottom(FormatBorder::Thin);
    }
    if b.left {
        f = f.set_border_left(FormatBorder::Thin);
    }
    if b.right {
        f = f.set_border_right(FormatBorder::Thin);
    }
    f
}
