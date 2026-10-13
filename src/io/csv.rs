//! CSV/TSV import and export.

use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{Book, NumFmt, Sheet, interpret_input};

pub(super) fn load(path: &Path) -> Result<Book> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter(path))
        .from_path(path)
        .with_context(|| format!("Couldn't open {}", path.display()))?;
    let mut cells = Vec::new();
    for (r, rec) in rdr.records().enumerate() {
        let rec = rec.context("Malformed CSV")?;
        for (c, field) in rec.iter().enumerate() {
            if let Some(cell) = interpret_input(field, NumFmt::General) {
                cells.push(((r as u32, c as u32), cell));
            }
        }
    }
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Sheet1");
    let name: String = name
        .chars()
        .filter(|c| !"[]:*?/\\".contains(*c))
        .take(31)
        .collect();
    Ok(Book::from_sheets(vec![(name, cells)]))
}

pub(super) fn save(sheet: &Sheet, path: &Path) -> Result<()> {
    let mut w = csv::WriterBuilder::new()
        .delimiter(delimiter(path))
        .from_path(path)
        .with_context(|| format!("Couldn't write {}", path.display()))?;
    for r in 0..sheet.n_rows {
        let row: Vec<String> = (0..sheet.n_cols).map(|c| sheet.display((r, c))).collect();
        w.write_record(&row)?;
    }
    w.flush()?;
    Ok(())
}

/// Tab for `.tsv`, comma otherwise.
fn delimiter(path: &Path) -> u8 {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("tsv") => b'\t',
        _ => b',',
    }
}
