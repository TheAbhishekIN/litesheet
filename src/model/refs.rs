//! A1-style reference helpers and relative-reference shifting for formulas.

/// Zero-based column index to letters: 0 -> "A", 25 -> "Z", 26 -> "AA".
pub fn col_name(mut col: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// Letters to zero-based column index ("A" -> 0). Case-insensitive.
pub fn col_index(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n: u32 = 0;
    for b in letters.bytes() {
        let b = b.to_ascii_uppercase();
        if !b.is_ascii_uppercase() {
            return None;
        }
        n = n * 26 + (b - b'A' + 1) as u32;
    }
    Some(n - 1)
}

/// Zero-based (row, col) to "A1".
pub fn cell_name(row: u32, col: u32) -> String {
    format!("{}{}", col_name(col), row + 1)
}

/// Parse "B12" into zero-based (row, col).
pub fn parse_cell(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    let col = col_index(letters)?;
    let row: u32 = digits.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((row - 1, col))
}

/// Shift every relative A1 reference in `formula` by (`dr`, `dc`), leaving
/// `$`-anchored parts, string literals and function names untouched.
/// References that would move off the sheet become `#REF!`.
pub fn shift_formula(formula: &str, dr: i64, dc: i64) -> String {
    if !formula.starts_with('=') || (dr == 0 && dc == 0) {
        return formula.to_string();
    }
    let chars: Vec<char> = formula.chars().collect();
    let mut out = String::with_capacity(formula.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // String literal: copy verbatim.
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '"' {
                    if i + 1 < chars.len() && chars[i + 1] == '"' {
                        out.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // Quoted sheet name: copy verbatim.
        if c == '\'' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                i += 1;
                if chars[i - 1] == '\'' {
                    break;
                }
            }
            continue;
        }
        let prev_is_word = i > 0 && is_word(chars[i - 1]);
        if !prev_is_word && (c == '$' || c.is_ascii_alphabetic()) {
            if let Some((len, text)) = try_shift_ref(&chars[i..], dr, dc) {
                out.push_str(&text);
                i += len;
                continue;
            }
            // Not a reference: copy the whole identifier so we don't match inside it.
            let start = i;
            while i < chars.len() && (is_word(chars[i]) || chars[i] == '$') {
                i += 1;
            }
            if i == start {
                i += 1;
            }
            out.extend(&chars[start..i]);
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The sheet name as it must appear in a formula: quoted (`'My Sheet'`) when
/// it isn't a plain identifier or could be mistaken for a cell reference.
pub fn quote_sheet(name: &str) -> String {
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        && parse_cell(name).is_none()
        && !name.eq_ignore_ascii_case("true")
        && !name.eq_ignore_ascii_case("false");
    if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// Point every reference to sheet `old` in `formula` at sheet `new` instead
/// (`Old!A1`, `'Old'!A1:B2`, …). Sheet names match case-insensitively, as in Excel.
pub fn rename_sheet_refs(formula: &str, old: &str, new: &str) -> String {
    if !formula.starts_with('=') {
        return formula.to_string();
    }
    let chars: Vec<char> = formula.chars().collect();
    let replacement = quote_sheet(new);
    let mut out = String::with_capacity(formula.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            // String literal: copy verbatim.
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                i += 1;
                if chars[i - 1] == '"' {
                    if chars.get(i) == Some(&'"') {
                        out.push('"');
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        if c == '\'' {
            // Quoted sheet name: read up to the closing quote ('' escapes a quote).
            let start = i;
            let mut name = String::new();
            i += 1;
            while i < chars.len() {
                if chars[i] == '\'' {
                    if chars.get(i + 1) == Some(&'\'') {
                        name.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                name.push(chars[i]);
                i += 1;
            }
            if chars.get(i) == Some(&'!') && name.eq_ignore_ascii_case(old) {
                out.push_str(&replacement);
            } else {
                out.extend(&chars[start..i]);
            }
            continue;
        }
        let prev_is_word = i > 0 && is_word(chars[i - 1]);
        if !prev_is_word && (c.is_alphabetic() || c == '_') {
            let start = i;
            while i < chars.len() && (is_word(chars[i]) || chars[i].is_alphanumeric()) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if chars.get(i) == Some(&'!') && word.eq_ignore_ascii_case(old) {
                out.push_str(&replacement);
            } else {
                out.push_str(&word);
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Which way a structural change runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Rows,
    Cols,
}

/// Rows or columns inserted (`count > 0`) or deleted (`count < 0`) at the
/// zero-based index `at` of sheet `sheet`.
#[derive(Clone, Copy, Debug)]
pub struct Structural<'a> {
    pub sheet: &'a str,
    pub axis: Axis,
    pub at: u32,
    pub count: i64,
}

impl Structural<'_> {
    /// Where index `i` ends up, or `None` if it was deleted.
    pub fn map_index(&self, i: u32) -> Option<u32> {
        let (at, n) = (self.at as i64, self.count);
        let i = i as i64;
        if n >= 0 {
            Some(if i >= at { i + n } else { i } as u32)
        } else if i < at {
            Some(i as u32)
        } else if i < at - n {
            None
        } else {
            Some((i + n) as u32)
        }
    }

    /// New (start, end) of an inclusive span, or `None` if all of it was deleted.
    fn map_span(&self, lo: u32, hi: u32) -> Option<(u32, u32)> {
        if self.count >= 0 {
            return Some((self.map_index(lo)?, self.map_index(hi)?));
        }
        let (at, end) = (self.at as i64, self.at as i64 - self.count);
        let (l, h) = (lo as i64, hi as i64);
        if l >= at && h < end {
            return None;
        }
        let nl = if l >= end { l + self.count } else { l.min(at) };
        let nh = if h >= end {
            h + self.count
        } else if h >= at {
            at - 1
        } else {
            h
        };
        Some((nl as u32, nh as u32))
    }
}

/// One end of a reference: a cell (`$B$2`), a whole column (`B`) or a whole row (`2`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Cell {
        col: u32,
        col_abs: bool,
        row: u32,
        row_abs: bool,
    },
    Col {
        col: u32,
        abs: bool,
    },
    Row {
        row: u32,
        abs: bool,
    },
}

impl Part {
    fn index(&self, axis: Axis) -> Option<u32> {
        match (self, axis) {
            (Part::Cell { row, .. }, Axis::Rows) | (Part::Row { row, .. }, Axis::Rows) => {
                Some(*row)
            }
            (Part::Cell { col, .. }, Axis::Cols) | (Part::Col { col, .. }, Axis::Cols) => {
                Some(*col)
            }
            _ => None,
        }
    }

    fn with_index(self, axis: Axis, i: u32) -> Part {
        match (self, axis) {
            (
                Part::Cell {
                    col,
                    col_abs,
                    row_abs,
                    ..
                },
                Axis::Rows,
            ) => Part::Cell {
                col,
                col_abs,
                row: i,
                row_abs,
            },
            (
                Part::Cell {
                    row,
                    row_abs,
                    col_abs,
                    ..
                },
                Axis::Cols,
            ) => Part::Cell {
                col: i,
                col_abs,
                row,
                row_abs,
            },
            (Part::Row { abs, .. }, Axis::Rows) => Part::Row { row: i, abs },
            (Part::Col { abs, .. }, Axis::Cols) => Part::Col { col: i, abs },
            (p, _) => p,
        }
    }

    fn write(&self, out: &mut String) {
        let d = |abs: bool| if abs { "$" } else { "" };
        match *self {
            Part::Cell {
                col,
                col_abs,
                row,
                row_abs,
            } => out.push_str(&format!(
                "{}{}{}{}",
                d(col_abs),
                col_name(col),
                d(row_abs),
                row + 1
            )),
            Part::Col { col, abs } => out.push_str(&format!("{}{}", d(abs), col_name(col))),
            Part::Row { row, abs } => out.push_str(&format!("{}{}", d(abs), row + 1)),
        }
    }
}

/// Read `$?LETTERS$?DIGITS`, `$?LETTERS` or `$?DIGITS` at `i`.
fn read_part(s: &[char], mut i: usize) -> Option<(Part, usize)> {
    let start = i;
    let col_abs = s.get(i) == Some(&'$');
    if col_abs {
        i += 1;
    }
    let ls = i;
    while i < s.len() && s[i].is_ascii_alphabetic() && i - ls < 4 {
        i += 1;
    }
    let letters: String = s[ls..i].iter().collect();
    let row_abs = !letters.is_empty() && s.get(i) == Some(&'$');
    if row_abs {
        i += 1;
    }
    let ds = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let digits: String = s[ds..i].iter().collect();
    let part = match (letters.is_empty(), digits.is_empty()) {
        (false, false) => {
            let row: u32 = digits.parse().ok()?;
            Part::Cell {
                col: col_index(&letters)?,
                col_abs,
                row: row.checked_sub(1)?,
                row_abs,
            }
        }
        (false, true) if !row_abs => Part::Col {
            col: col_index(&letters)?,
            abs: col_abs,
        },
        (true, false) => {
            let row: u32 = digits.parse().ok()?;
            Part::Row {
                row: row.checked_sub(1)?,
                abs: col_abs,
            }
        }
        _ => return None,
    };
    // The part must end here: not run into a longer name or a function call.
    if s.get(i)
        .is_some_and(|&c| is_word(c) || c == '(' || c == '!')
        || i == start
    {
        return None;
    }
    Some((part, i))
}

/// A reference starting at `i`: optional sheet prefix, then a cell, a cell
/// range, a column range (`A:C`) or a row range (`1:3`).
/// Returns (sheet name, prefix text, first part, second part, end index).
#[allow(clippy::type_complexity)]
fn read_reference(
    s: &[char],
    i: usize,
) -> Option<(Option<String>, String, Part, Option<Part>, usize)> {
    let (mut sheet, mut prefix, mut j) = (None, String::new(), i);
    if s[i] == '\'' {
        let mut k = i + 1;
        let mut name = String::new();
        while k < s.len() {
            if s[k] == '\'' {
                if s.get(k + 1) == Some(&'\'') {
                    name.push('\'');
                    k += 2;
                    continue;
                }
                break;
            }
            name.push(s[k]);
            k += 1;
        }
        if s.get(k + 1) != Some(&'!') {
            return None;
        }
        prefix = s[i..k + 2].iter().collect();
        sheet = Some(name);
        j = k + 2;
    } else if s[i].is_alphabetic() || s[i] == '_' {
        let mut k = i;
        while k < s.len() && (is_word(s[k]) || s[k].is_alphanumeric()) {
            k += 1;
        }
        if s.get(k) == Some(&'!') {
            sheet = Some(s[i..k].iter().collect());
            prefix = s[i..=k].iter().collect();
            j = k + 1;
        }
    }
    let (a, end) = read_part(s, j)?;
    if s.get(end) == Some(&':')
        && let Some((b, end2)) = read_part(s, end + 1)
        && std::mem::discriminant(&a) == std::mem::discriminant(&b)
    {
        return Some((sheet, prefix, a, Some(b), end2));
    }
    match a {
        Part::Cell { .. } => Some((sheet, prefix, a, None, end)),
        _ => None, // a bare column or row only makes sense in a range
    }
}

/// Rewrite the references in `formula` (which lives on sheet `own_sheet`)
/// after rows or columns were inserted or deleted. Unlike copy/paste, this
/// moves absolute (`$`) references too, grows or shrinks ranges, and turns
/// references to deleted cells into `#REF!`.
pub fn adjust_for_structure(formula: &str, own_sheet: &str, change: &Structural) -> String {
    if !formula.starts_with('=') {
        return formula.to_string();
    }
    let s: Vec<char> = formula.chars().collect();
    let mut out = String::with_capacity(formula.len() + 8);
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < s.len() {
                out.push(s[i]);
                i += 1;
                if s[i - 1] == '"' {
                    if s.get(i) == Some(&'"') {
                        out.push('"');
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        let starts_token = i == 0 || !(is_word(s[i - 1]) || s[i - 1] == '$');
        if starts_token && (c == '\'' || c == '$' || c.is_alphanumeric() || c == '_') {
            if let Some((sheet, prefix, a, b, end)) = read_reference(&s, i) {
                let target = sheet.as_deref().unwrap_or(own_sheet);
                out.push_str(&prefix);
                if !target.eq_ignore_ascii_case(change.sheet) {
                    a.write(&mut out);
                } else {
                    match adjust_parts(a, b, change) {
                        Some((na, nb)) => {
                            na.write(&mut out);
                            if let Some(nb) = nb {
                                out.push(':');
                                nb.write(&mut out);
                            }
                        }
                        None => {
                            out.truncate(out.len() - prefix.len());
                            out.push_str("#REF!");
                        }
                    }
                    i = end;
                    continue;
                }
                if let Some(b) = b {
                    out.push(':');
                    b.write(&mut out);
                }
                i = end;
                continue;
            }
            // Not a reference: copy the whole word so we don't match inside it.
            let start = i;
            i += 1;
            while i < s.len() && (is_word(s[i]) || s[i].is_alphanumeric()) {
                i += 1;
            }
            out.extend(&s[start..i]);
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn adjust_parts(a: Part, b: Option<Part>, ch: &Structural) -> Option<(Part, Option<Part>)> {
    let Some(ai) = a.index(ch.axis) else {
        return Some((a, b)); // e.g. a column range when rows change
    };
    match b {
        None => Some((a.with_index(ch.axis, ch.map_index(ai)?), None)),
        Some(b) => {
            let bi = b.index(ch.axis)?;
            let (lo, hi) = (ai.min(bi), ai.max(bi));
            let (nlo, nhi) = ch.map_span(lo, hi)?;
            let (na, nb) = if ai <= bi { (nlo, nhi) } else { (nhi, nlo) };
            Some((a.with_index(ch.axis, na), Some(b.with_index(ch.axis, nb))))
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// Try to read a cell reference at the start of `s`. Returns consumed length and replacement.
fn try_shift_ref(s: &[char], dr: i64, dc: i64) -> Option<(usize, String)> {
    let mut i = 0;
    let col_abs = s.get(i) == Some(&'$');
    if col_abs {
        i += 1;
    }
    let ls = i;
    while i < s.len() && s[i].is_ascii_alphabetic() && i - ls < 4 {
        i += 1;
    }
    let letters: String = s[ls..i].iter().collect();
    let col = col_index(&letters)?;
    let row_abs = s.get(i) == Some(&'$');
    if row_abs {
        i += 1;
    }
    let ds = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    if i == ds {
        return None;
    }
    // Must not continue as an identifier or be a function call / sheet name.
    if let Some(&next) = s.get(i)
        && (is_word(next) || next == '(' || next == '!')
    {
        return None;
    }
    let row: i64 = s[ds..i].iter().collect::<String>().parse().ok()?;
    let new_col = if col_abs { col as i64 } else { col as i64 + dc };
    let new_row = if row_abs { row } else { row + dr };
    if new_col < 0 || new_row < 1 {
        return Some((i, "#REF!".to_string()));
    }
    Some((
        i,
        format!(
            "{}{}{}{}",
            if col_abs { "$" } else { "" },
            col_name(new_col as u32),
            if row_abs { "$" } else { "" },
            new_row
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(col_name(702), "AAA");
        for i in 0..2000 {
            assert_eq!(col_index(&col_name(i)), Some(i));
        }
        assert_eq!(parse_cell("b12"), Some((11, 1)));
        assert_eq!(parse_cell("$C$3"), Some((2, 2)));
        assert_eq!(parse_cell("A0"), None);
    }

    #[test]
    fn shifting() {
        assert_eq!(shift_formula("=A1+B2", 1, 0), "=A2+B3");
        assert_eq!(shift_formula("=$A1+B$2", 2, 1), "=$A3+C$2");
        assert_eq!(shift_formula("=SUM(A1:A10)", 0, 2), "=SUM(C1:C10)");
        assert_eq!(shift_formula("=\"A1\"&A1", 1, 0), "=\"A1\"&A2");
        assert_eq!(shift_formula("=LOG10(A1)", 1, 0), "=LOG10(A2)");
        assert_eq!(shift_formula("=Sheet2!A1", 1, 0), "=Sheet2!A2");
        assert_eq!(shift_formula("='My Sheet'!B2*2", 0, 1), "='My Sheet'!C2*2");
        assert_eq!(shift_formula("=A1", -1, 0), "=#REF!");
        assert_eq!(shift_formula("hello A1", 1, 0), "hello A1");
    }

    #[test]
    fn renaming_sheets() {
        assert_eq!(quote_sheet("Data"), "Data");
        assert_eq!(quote_sheet("My Data"), "'My Data'");
        assert_eq!(quote_sheet("2024"), "'2024'");
        assert_eq!(quote_sheet("AB12"), "'AB12'");
        assert_eq!(quote_sheet("Bob's"), "'Bob''s'");
        assert_eq!(
            rename_sheet_refs("=Data!A1*2", "Data", "Inputs"),
            "=Inputs!A1*2"
        );
        assert_eq!(
            rename_sheet_refs("=SUM(data!A1:A9)", "Data", "My Inputs"),
            "=SUM('My Inputs'!A1:A9)"
        );
        assert_eq!(
            rename_sheet_refs("='My Data'!B2+Other!B2", "my data", "D"),
            "=D!B2+Other!B2"
        );
        assert_eq!(
            rename_sheet_refs("=\"Data!A1\"&Data!A1", "Data", "X"),
            "=\"Data!A1\"&X!A1"
        );
        assert_eq!(
            rename_sheet_refs("=DataSet!A1+Data2!A1", "Data", "X"),
            "=DataSet!A1+Data2!A1"
        );
        assert_eq!(
            rename_sheet_refs("='Bob''s'!A1", "Bob's", "Al's"),
            "='Al''s'!A1"
        );
        assert_eq!(rename_sheet_refs("=Data!A:A", "Data", "X"), "=X!A:A");
    }

    #[test]
    fn structural_adjustment() {
        let ins = |f: &str| {
            adjust_for_structure(
                f,
                "S",
                &Structural {
                    sheet: "S",
                    axis: Axis::Rows,
                    at: 2,
                    count: 2,
                },
            )
        };
        let del = |f: &str| {
            adjust_for_structure(
                f,
                "S",
                &Structural {
                    sheet: "S",
                    axis: Axis::Rows,
                    at: 2,
                    count: -2,
                },
            )
        };
        let ins_col = |f: &str| {
            adjust_for_structure(
                f,
                "S",
                &Structural {
                    sheet: "S",
                    axis: Axis::Cols,
                    at: 1,
                    count: 1,
                },
            )
        };
        let del_col = |f: &str| {
            adjust_for_structure(
                f,
                "S",
                &Structural {
                    sheet: "S",
                    axis: Axis::Cols,
                    at: 1,
                    count: -1,
                },
            )
        };

        assert_eq!(ins("=A5"), "=A7");
        assert_eq!(ins("=A2"), "=A2");
        assert_eq!(ins("=$A$5*2"), "=$A$7*2");
        assert_eq!(ins("=SUM(A1:A9)"), "=SUM(A1:A11)");
        assert_eq!(ins("=SUM(A3:A4)"), "=SUM(A5:A6)");
        assert_eq!(ins("=SUM(A:A)"), "=SUM(A:A)");
        assert_eq!(ins("=SUM(3:5)"), "=SUM(5:7)");
        assert_eq!(ins("=\"A5\"&A5"), "=\"A5\"&A7");
        assert_eq!(ins("=LOG10(A5)"), "=LOG10(A7)");
        assert_eq!(ins("=Other!A5+S!A5+'S'!A5"), "=Other!A5+S!A7+'S'!A7");

        assert_eq!(del("=A5"), "=A3");
        assert_eq!(del("=A3"), "=#REF!");
        assert_eq!(del("=S!A4+1"), "=#REF!+1");
        assert_eq!(del("=SUM(A1:A9)"), "=SUM(A1:A7)");
        assert_eq!(del("=SUM(A3:A4)"), "=SUM(#REF!)");
        assert_eq!(del("=SUM(A2:A3)"), "=SUM(A2:A2)");
        assert_eq!(del("=SUM(A4:A9)"), "=SUM(A3:A7)");
        assert_eq!(del("=A2"), "=A2");

        assert_eq!(ins_col("=A1+B1+C1"), "=A1+C1+D1");
        assert_eq!(ins_col("=SUM(A1:C1)"), "=SUM(A1:D1)");
        assert_eq!(ins_col("=SUM(B:C)"), "=SUM(C:D)");
        assert_eq!(ins_col("=SUM(1:1)"), "=SUM(1:1)");
        assert_eq!(del_col("=A1+B1+C1"), "=A1+#REF!+B1");
        assert_eq!(del_col("=SUM(A1:C1)"), "=SUM(A1:B1)");
        assert_eq!(del_col("=TRUE"), "=TRUE");
        assert_eq!(del_col("=2+3"), "=2+3");
    }
}
