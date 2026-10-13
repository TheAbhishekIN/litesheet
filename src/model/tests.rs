//! Workbook-level tests: engine integration, undo/redo and formatting.

use super::*;
#[test]
fn engine_roundtrip() {
    let mut b = Book::new_blank();
    b.set_input(0, (0, 0), "2");
    b.set_input(0, (1, 0), "3");
    b.set_input(0, (2, 0), "=SUM(A1:A2)*2");
    assert_eq!(b.sheets[0].display((2, 0)), "10");
    b.set_input(0, (0, 0), "10");
    assert_eq!(b.sheets[0].display((2, 0)), "26");
    b.undo();
    assert_eq!(b.sheets[0].display((2, 0)), "10");
    b.redo();
    assert_eq!(b.sheets[0].display((2, 0)), "26");
    b.set_input(0, (3, 0), "=A1/0");
    assert_eq!(b.sheets[0].display((3, 0)), "#DIV/0!");
    b.set_input(0, (4, 0), "=VLOOKUP(10,A1:A3,1,FALSE)");
    assert_eq!(b.sheets[0].display((4, 0)), "10");
    b.set_input(0, (5, 0), "=UPPER(\"abc\")&\"-\"&TEXT(1.5,\"0.00\")");
    assert_eq!(b.sheets[0].display((5, 0)), "ABC-1.50");
    b.set_input(0, (0, 0), "");
    assert_eq!(b.sheets[0].display((2, 0)), "6");
}

#[test]
fn cross_sheet() {
    let mut b = Book::new_blank();
    let s2 = b.add_sheet("Data");
    b.set_input(s2, (0, 0), "7");
    b.set_input(0, (0, 0), "=Data!A1*3");
    assert_eq!(b.sheets[0].display((0, 0)), "21");
    b.rename_sheet(s2, "Inputs").unwrap();
    b.set_input(0, (1, 0), "=Inputs!A1+1");
    assert_eq!(b.sheets[0].display((1, 0)), "8");
}

#[test]
fn bad_formula() {
    let mut b = Book::new_blank();
    b.set_input(0, (0, 0), "=SUM(");
    assert!(matches!(b.sheets[0].value((0, 0)), Value::Error(_)));
}

#[test]
fn formatting() {
    assert_eq!(fmt_number(0.1 + 0.2), "0.3");
    assert_eq!(fmt_number(1234567.0), "1234567");
    assert_eq!(fmt_number(-2.5), "-2.5");
    assert_eq!(fmt_number(1.0 / 3.0), "0.333333333333333");
    let d = parse_date("2024-05-01").unwrap();
    assert_eq!(d, 45413.0);
    assert_eq!(format_value(&Value::Number(d), NumFmt::Date), "2024-05-01");
    assert_eq!(
        format_value(&Value::Number(0.155), NumFmt::Percent),
        "15.5%"
    );
    let c = interpret_input("15%", NumFmt::General).unwrap();
    assert_eq!((c.raw.as_str(), c.fmt), ("0.15", NumFmt::Percent));
}

#[test]
fn rename_sheet_updates_formulas_and_undoes() {
    let mut b = Book::new_blank();
    let data = b.add_sheet("Data");
    b.set_input(data, (0, 0), "7");
    b.set_input(0, (0, 0), "=Data!A1*3");
    b.set_input(data, (1, 0), "=SUM(Data!A1:A1)+1");
    b.rename_sheet(data, "My Inputs").unwrap();
    assert_eq!(b.sheets[0].cells[&(0, 0)].raw, "='My Inputs'!A1*3");
    assert_eq!(b.sheets[0].display((0, 0)), "21");
    assert_eq!(
        b.sheets[data].cells[&(1, 0)].raw,
        "=SUM('My Inputs'!A1:A1)+1"
    );
    assert_eq!(b.undo_label(), Some("Rename sheet"));

    b.undo();
    assert_eq!(b.sheets[data].name, "Data");
    assert_eq!(b.sheets[0].cells[&(0, 0)].raw, "=Data!A1*3");
    assert_eq!(b.sheets[0].display((0, 0)), "21");

    b.redo();
    assert_eq!(b.sheets[data].name, "My Inputs");
    assert_eq!(b.sheets[0].display((0, 0)), "21");
    b.set_input(data, (0, 0), "10");
    assert_eq!(b.sheets[0].display((0, 0)), "30");
}

#[test]
fn insert_and_delete_rows_and_columns() {
    let mut b = Book::new_blank();
    let other = b.add_sheet("Other");
    for (r, v) in ["1", "2", "3", "4"].iter().enumerate() {
        b.set_input(0, (r as u32, 0), v);
    }
    b.set_input(0, (4, 0), "=SUM(A1:A4)");
    b.set_input(0, (0, 1), "=A4*10");
    b.set_input(other, (0, 0), "=Sheet1!A3+Sheet1!A5");
    b.sheets[0].col_widths.insert(1, 150.0);

    // Two rows above row 3: values move down, formulas follow.
    b.insert_rows(0, 2, 2).unwrap();
    assert_eq!(b.sheets[0].display((6, 0)), "10");
    assert_eq!(b.sheets[0].cells[&(6, 0)].raw, "=SUM(A1:A6)");
    assert_eq!(b.sheets[0].cells[&(0, 1)].raw, "=A6*10");
    assert_eq!(b.sheets[other].cells[&(0, 0)].raw, "=Sheet1!A5+Sheet1!A7");
    assert_eq!(b.sheets[other].display((0, 0)), "13");
    assert_eq!(b.sheets[0].display((2, 0)), "");
    assert_eq!(b.undo_label(), Some("Insert 2 rows"));

    // Undo puts everything back.
    b.undo();
    assert_eq!(b.sheets[0].cells[&(4, 0)].raw, "=SUM(A1:A4)");
    assert_eq!(b.sheets[other].display((0, 0)), "13");
    b.redo();
    assert_eq!(b.sheets[0].display((6, 0)), "10");
    b.undo();

    // Delete row 3 (value 3): the sum shrinks, a direct reference breaks.
    b.delete_rows(0, 2, 1).unwrap();
    assert_eq!(b.sheets[0].cells[&(3, 0)].raw, "=SUM(A1:A3)");
    assert_eq!(b.sheets[0].display((3, 0)), "7");
    assert_eq!(b.sheets[other].cells[&(0, 0)].raw, "=#REF!+Sheet1!A4");
    b.undo();

    // Insert a column before B: B's width moves to C.
    b.insert_cols(0, 1, 1).unwrap();
    assert_eq!(b.sheets[0].cells[&(0, 2)].raw, "=A4*10");
    assert_eq!(b.sheets[0].col_widths.get(&2), Some(&150.0));
    assert_eq!(b.sheets[0].col_widths.get(&1), None);
    b.undo();
    assert_eq!(b.sheets[0].col_widths.get(&1), Some(&150.0));

    // Delete column A: formulas pointing at it break.
    b.delete_cols(0, 0, 1).unwrap();
    assert_eq!(b.sheets[0].cells[&(0, 0)].raw, "=#REF!*10");
    assert!(matches!(b.sheets[0].value((0, 0)), Value::Error(_)));
}
