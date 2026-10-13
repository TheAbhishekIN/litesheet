//! Reading and writing workbook files. The format is chosen from the file
//! extension: `.csv`/`.tsv`/`.txt` are delimited text, anything else is a
//! spreadsheet read by calamine and written as `.xlsx`.

mod csv;
mod xlsx;

use std::path::Path;

use anyhow::Result;

use crate::model::Book;

pub fn is_csv(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("csv" | "tsv" | "txt")
    )
}

pub fn load(path: &Path) -> Result<Book> {
    let mut book = if is_csv(path) {
        csv::load(path)?
    } else {
        xlsx::load(path)?
    };
    book.path = Some(path.to_path_buf());
    book.dirty = false;
    Ok(book)
}

pub fn save(book: &Book, active_sheet: usize, path: &Path) -> Result<()> {
    if is_csv(path) {
        csv::save(&book.sheets[active_sheet], path)
    } else {
        xlsx::save(book, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_and_csv_roundtrip() {
        let dir = std::env::temp_dir().join(format!("litesheet-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut b = Book::new_blank();
        b.set_input(0, (0, 0), "Item");
        b.set_input(0, (0, 1), "Price");
        b.set_input(0, (1, 0), "Apple");
        b.set_input(0, (1, 1), "1.25");
        b.set_input(0, (2, 0), "0042");
        b.set_input(0, (2, 1), "2");
        b.set_input(0, (3, 1), "=SUM(B2:B3)");
        b.set_input(0, (4, 1), "2024-05-01");
        b.sheets[0].col_widths.insert(0, 150.0);
        let s2 = b.add_sheet("Other");
        b.set_input(s2, (0, 0), "=Sheet1!B4*2");

        let xlsx = dir.join("t.xlsx");
        save(&b, 0, &xlsx).unwrap();
        let l = load(&xlsx).unwrap();
        assert_eq!(l.sheets.len(), 2);
        let s = &l.sheets[0];
        assert_eq!(s.display((1, 0)), "Apple");
        assert_eq!(s.raw((3, 1)), "=SUM(B2:B3)");
        assert_eq!(s.display((3, 1)), "3.25");
        assert_eq!(s.display((4, 1)), "2024-05-01");
        assert_eq!(l.sheets[1].display((0, 0)), "6.5");

        let csv = dir.join("t.csv");
        save(&b, 0, &csv).unwrap();
        let l = load(&csv).unwrap();
        assert_eq!(l.sheets[0].display((3, 1)), "3.25");
        assert_eq!(l.sheets[0].display((0, 1)), "Price");
        std::fs::remove_dir_all(&dir).ok();
    }
}
