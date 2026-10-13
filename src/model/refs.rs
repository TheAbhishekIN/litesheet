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
}
