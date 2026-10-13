//! Workbook-level tests: engine integration, undo/redo and formatting.

use super::cell::Currency;
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
        format_value(&Value::Number(0.155), NumFmt::Percent { decimals: None }),
        "15.5%"
    );
    let c = interpret_input("15%", NumFmt::General).unwrap();
    assert_eq!(
        (c.raw.as_str(), c.fmt),
        ("0.15", NumFmt::Percent { decimals: None })
    );
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

#[test]
fn number_formats_display() {
    let n = |v: f64, f: NumFmt| format_value(&Value::Number(v), f);
    let number = |decimals, thousands| NumFmt::Number {
        decimals,
        thousands,
    };
    assert_eq!(n(1234.567, number(2, true)), "1,234.57");
    assert_eq!(n(-1234567.0, number(0, true)), "-1,234,567");
    assert_eq!(n(-0.001, number(2, false)), "0.00");
    assert_eq!(n(0.5, number(0, false)), "1");
    assert_eq!(n(2.5, number(0, false)), "3");
    assert_eq!(n(1.005, number(2, false)), "1.01");
    assert_eq!(n(-1.005, number(2, false)), "-1.01");
    let cur = |symbol| NumFmt::Currency {
        decimals: 2,
        symbol,
    };
    assert_eq!(n(1250.5, cur(Currency::Dollar)), "$1,250.50");
    assert_eq!(n(-12.0, cur(Currency::Euro)), "-€12.00");
    assert_eq!(n(99999.999, cur(Currency::Rupee)), "₹100,000.00");
    assert_eq!(n(0.1234, NumFmt::Percent { decimals: Some(1) }), "12.3%");
    assert_eq!(n(0.155, NumFmt::Percent { decimals: None }), "15.5%");
    assert_eq!(n(12345.0, NumFmt::Scientific { decimals: 2 }), "1.23E+04");
    assert_eq!(n(0.000123, NumFmt::Scientific { decimals: 1 }), "1.2E-04");
    assert_eq!(n(99999.0, NumFmt::Scientific { decimals: 2 }), "1.00E+05");
    assert_eq!(n(0.0, NumFmt::Scientific { decimals: 2 }), "0.00E+00");
    assert_eq!(n(0.5 + 1.0 / 86400.0, NumFmt::Time), "12:00:01");
    assert_eq!(n(45413.75, NumFmt::Time), "18:00:00");
}

#[test]
fn typing_recognises_formats() {
    let i = |s: &str, prev| interpret_input(s, prev).map(|c| (c.raw, c.fmt));
    let g = NumFmt::General;
    assert_eq!(
        i("$1,250.50", g),
        Some((
            "1250.5".into(),
            NumFmt::Currency {
                decimals: 2,
                symbol: Currency::Dollar
            }
        ))
    );
    assert_eq!(
        i("-€12", g),
        Some((
            "-12".into(),
            NumFmt::Currency {
                decimals: 0,
                symbol: Currency::Euro
            }
        ))
    );
    assert_eq!(
        i("1,234", g),
        Some((
            "1234".into(),
            NumFmt::Number {
                decimals: 0,
                thousands: true
            }
        ))
    );
    assert_eq!(i("1,23", g), Some(("1,23".into(), g))); // not a grouped number
    assert_eq!(i("14:30", g), Some((fmt_number(14.5 / 24.0), NumFmt::Time)));
    assert_eq!(
        i("2:30 PM", g),
        Some((fmt_number(14.5 / 24.0), NumFmt::Time))
    );
    assert_eq!(i("12:00 AM", g), Some(("0".into(), NumFmt::Time)));
    assert_eq!(i("25:00", g), Some(("25:00".into(), g)));
    // A plain number keeps the cell's format; in a percent cell it means percent.
    let cur = NumFmt::Currency {
        decimals: 2,
        symbol: Currency::Pound,
    };
    assert_eq!(i("42", cur), Some(("42".into(), cur)));
    let pct = NumFmt::Percent { decimals: Some(0) };
    assert_eq!(i("15", pct), Some(("0.15".into(), pct)));
    // Text cells keep exactly what was typed.
    assert_eq!(
        i("00042", NumFmt::Text),
        Some(("00042".into(), NumFmt::Text))
    );
    assert_eq!(i("=1+1", NumFmt::Text), Some(("=1+1".into(), NumFmt::Text)));
}

#[test]
fn text_and_formatted_empty_cells() {
    let mut b = Book::new_blank();
    let text = Cell {
        raw: "00042".into(),
        fmt: NumFmt::Text,
        ..Default::default()
    };
    b.apply("t", vec![(0, (0, 0), Some(text))]);
    assert_eq!(b.sheets[0].display((0, 0)), "00042");
    assert!(matches!(b.sheets[0].value((0, 0)), Value::Text(_)));
    let formula_text = Cell {
        raw: "=1+1".into(),
        fmt: NumFmt::Text,
        ..Default::default()
    };
    b.apply("t", vec![(0, (1, 0), Some(formula_text))]);
    assert_eq!(b.sheets[0].display((1, 0)), "=1+1");

    // An empty cell can carry a format; typing a number there uses it.
    let fmt = NumFmt::Currency {
        decimals: 2,
        symbol: Currency::Dollar,
    };
    b.apply(
        "f",
        vec![(
            0,
            (5, 5),
            Some(Cell {
                raw: String::new(),
                fmt,
                ..Default::default()
            }),
        )],
    );
    assert_eq!(
        b.sheets[0].n_rows, 2,
        "formatted empty cells aren't part of the used range"
    );
    b.set_input(0, (5, 5), "7");
    assert_eq!(b.sheets[0].display((5, 5)), "$7.00");
}

#[test]
fn merge_unmerge_and_structure() {
    let mut b = Book::new_blank();
    b.set_input(0, (0, 0), "Title");
    b.set_input(0, (0, 1), "lost");
    b.set_input(0, (1, 0), "kept");
    b.merge(0, ((0, 0), (0, 2)), true);
    assert_eq!(b.sheets[0].merges, vec![((0, 0), (0, 2))]);
    assert_eq!(b.sheets[0].display((0, 1)), "");
    assert_eq!(b.sheets[0].cells[&(0, 0)].style.align, HAlign::Center);
    assert_eq!(b.sheets[0].merge_at((0, 2)), Some(((0, 0), (0, 2))));
    assert_eq!(b.sheets[0].merge_at((1, 0)), None);

    // Undo brings back the merged-away value and removes the merge.
    b.undo();
    assert!(b.sheets[0].merges.is_empty());
    assert_eq!(b.sheets[0].display((0, 1)), "lost");
    b.redo();

    // A new overlapping merge replaces the old one.
    b.merge(0, ((0, 1), (1, 3)), false);
    assert_eq!(b.sheets[0].merges, vec![((0, 1), (1, 3))]);
    b.undo();

    // Inserting a column inside the merge widens it; deleting rows moves it.
    b.insert_cols(0, 1, 1).unwrap();
    assert_eq!(b.sheets[0].merges, vec![((0, 0), (0, 3))]);
    b.insert_rows(0, 0, 2).unwrap();
    assert_eq!(b.sheets[0].merges, vec![((2, 0), (2, 3))]);
    // Deleting all but one of its columns dissolves it.
    b.delete_cols(0, 1, 3).unwrap();
    assert!(b.sheets[0].merges.is_empty());
    b.undo();
    assert_eq!(b.sheets[0].merges, vec![((2, 0), (2, 3))]);

    b.unmerge(0, ((2, 2), (2, 2)));
    assert!(b.sheets[0].merges.is_empty());
    assert_eq!(b.undo_label(), Some("Unmerge cells"));
}

#[test]
fn freeze_panes_follow_structure() {
    use crate::model::Freeze;
    let mut b = Book::new_blank();
    b.set_freeze(0, Freeze { rows: 2, cols: 1 });
    assert_eq!(b.undo_label(), Some("Freeze panes"));
    b.insert_rows(0, 0, 3).unwrap();
    assert_eq!(b.sheets[0].freeze, Freeze { rows: 5, cols: 1 });
    b.insert_rows(0, 9, 3).unwrap(); // below the line: no change
    assert_eq!(b.sheets[0].freeze.rows, 5);
    b.delete_rows(0, 3, 4).unwrap(); // rows 4-7: two of them were frozen
    assert_eq!(b.sheets[0].freeze.rows, 3);
    b.delete_cols(0, 0, 1).unwrap();
    assert_eq!(b.sheets[0].freeze, Freeze { rows: 3, cols: 0 });
    b.undo();
    b.undo();
    assert_eq!(b.sheets[0].freeze, Freeze { rows: 5, cols: 1 });
    b.set_freeze(0, Freeze::default());
    assert!(!b.sheets[0].freeze.is_active());
}

#[test]
fn charts_follow_structure_and_renames() {
    use crate::model::{Chart, ChartKind, DataRef};
    let mut b = Book::new_blank();
    for (r, (m, v)) in [("Month", "Sales"), ("Jan", "10"), ("Feb", "12")]
        .iter()
        .enumerate()
    {
        b.set_input(0, (r as u32, 0), m);
        b.set_input(0, (r as u32, 1), v);
    }
    let chart = Chart::from_block(&b.sheets[0], (0, 0), (2, 1), ChartKind::Column).unwrap();
    b.set_charts(0, vec![chart], "Insert chart");
    assert_eq!(b.undo_label(), Some("Insert chart"));

    b.insert_rows(0, 0, 1).unwrap();
    let c = &b.sheets[0].charts[0];
    assert_eq!(c.series[0].values, DataRef::new("Sheet1", (2, 1), (3, 1)));
    assert_eq!(c.anchor, (1, 3));
    assert_eq!(c.data(&b).series[0].1, vec![Some(10.0), Some(12.0)]);

    b.rename_sheet(0, "Data").unwrap();
    assert_eq!(b.sheets[0].charts[0].series[0].values.sheet, "Data");
    assert_eq!(b.sheets[0].charts[0].data(&b).categories, ["Jan", "Feb"]);
    b.undo();
    assert_eq!(b.sheets[0].charts[0].series[0].values.sheet, "Sheet1");

    // Deleting all of the chart's data columns removes the chart.
    b.delete_cols(0, 0, 2).unwrap();
    assert!(b.sheets[0].charts.is_empty());
    b.undo();
    assert_eq!(b.sheets[0].charts.len(), 1);
}
