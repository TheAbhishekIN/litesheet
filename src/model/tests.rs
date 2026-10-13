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
