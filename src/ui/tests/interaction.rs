//! Mouse and keyboard interaction: selection, editing, clipboard, sort,
//! filter and search.

use egui::{Event, Key, Modifiers, pos2};
use egui_kittest::kittest::Queryable;

use super::*;

#[test]
fn renders_initial() {
    let mut h = harness(SALES_CSV);
    shot(&mut h, "01-initial");
}

#[test]
fn select_edit_undo() {
    let mut h = harness(SALES_CSV);
    // Dates were recognised and right-aligned as numbers.
    assert!(matches!(
        h.state().sheet().value((1, 1)),
        crate::model::Value::Number(_)
    ));

    click(&mut h, cell(2, 1), Modifiers::NONE);
    assert_eq!(h.state().sel.cursor, (2, 1));

    // Type a formula into H2 and commit with Enter.
    click(&mut h, cell(1, 7), Modifiers::NONE);
    type_text(&mut h, "=E2*F2");
    assert!(h.state().edit.is_some());
    shot(&mut h, "02-editing");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().edit.is_none());
    assert_eq!(display(&h, 1, 7), "239.88");
    assert_eq!(h.state().sel.cursor, (2, 7));

    // Escape cancels an edit.
    type_text(&mut h, "oops");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(display(&h, 2, 7), "");

    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert_eq!(display(&h, 1, 7), "");
    key(&mut h, Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(display(&h, 1, 7), "239.88");

    // Arrow keys and shift-extend.
    key(&mut h, Key::ArrowDown, Modifiers::SHIFT);
    key(&mut h, Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(h.state().sel.name(), "H2:H4");
}

#[test]
fn point_mode_builds_references() {
    let mut h = harness(SALES_CSV);
    click(&mut h, cell(1, 7), Modifiers::NONE);
    type_text(&mut h, "=");
    click(&mut h, cell(1, 4), Modifiers::NONE);
    type_text(&mut h, "*");
    click(&mut h, cell(1, 5), Modifiers::NONE);
    assert_eq!(h.state().edit.as_ref().unwrap().buf, "=E2*F2");
    shot(&mut h, "03-pointing");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(display(&h, 1, 7), "239.88");
}

#[test]
fn fill_copy_paste() {
    let mut h = harness(SALES_CSV);
    click(&mut h, cell(1, 7), Modifiers::NONE);
    type_text(&mut h, "=E2*F2");
    key(&mut h, Key::Enter, Modifiers::NONE);
    // Select H2:H9 and fill down.
    click(&mut h, cell(1, 7), Modifiers::NONE);
    click(&mut h, cell(8, 7), Modifiers::SHIFT);
    key(&mut h, Key::D, Modifiers::COMMAND);
    assert_eq!(display(&h, 8, 7), "230");
    assert_eq!(h.state().book.sheets[0].edit_text((8, 7)), "=E9*F9");

    // Copy E2:F3 and paste at J2 (internal copy keeps formulas relative).
    click(&mut h, cell(1, 4), Modifiers::NONE);
    click(&mut h, cell(2, 5), Modifiers::SHIFT);
    h.event(Event::Copy);
    h.run_steps(2);
    let text = h.state().book.sheets[0].display((1, 4));
    assert_eq!(text, "12");
    click(&mut h, cell(1, 9), Modifiers::NONE);
    h.event(Event::Paste("12\t19.99\n3\t120\n".into()));
    h.run_steps(2);
    assert_eq!(display(&h, 2, 10), "120");
    assert_eq!(h.state().sel.name(), "J2:K3");

    // External text paste.
    click(&mut h, cell(10, 0), Modifiers::NONE);
    h.event(Event::Paste("a\tb\n1\t=A12+1".into()));
    h.run_steps(2);
    assert_eq!(display(&h, 11, 1), "2");
    shot(&mut h, "04-paste");
}

#[test]
fn sort_and_filter() {
    let mut h = harness(SALES_CSV);
    h.state_mut().sort_by(4, false);
    h.run_steps(2);
    assert_eq!(display(&h, 1, 4), "40");
    assert_eq!(display(&h, 0, 4), "Units");
    assert_eq!(display(&h, 8, 4), "2");

    // Filter buttons on, open the Region filter from its header button.
    key(&mut h, Key::L, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(h.state().filter_buttons);
    click(&mut h, pos2(50.0 + 300.0 - 9.0, 67.0), Modifiers::NONE);
    assert_eq!(h.state().filter_popup.as_ref().map(|p| p.col), Some(2));
    shot(&mut h, "05-filter-popup");
    {
        let p = h.state_mut().filter_popup.as_mut().unwrap();
        for v in p.values.iter_mut() {
            v.1 = v.0 == "West";
        }
    }
    h.get_by_label("Apply").click();
    h.run_steps(3);
    let rows = h.state().view().rows.clone().unwrap();
    assert_eq!(rows.len(), 4); // header + 3 West rows
    shot(&mut h, "06-filtered");

    // Arrow navigation skips hidden rows.
    click(&mut h, cell(1, 0), Modifiers::NONE);
    let r0 = h.state().sel.cursor.0;
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.state().sel.cursor.0, rows[2]);
    assert_ne!(r0, rows[2]);
}

#[test]
fn search() {
    let mut h = harness(SALES_CSV);
    key(&mut h, Key::F, Modifiers::COMMAND);
    type_text(&mut h, "widget");
    assert_eq!(h.state().view().matches.len(), 3);
    assert_eq!(h.state().sel.cursor, (1, 3));
    shot(&mut h, "07-search");
}

#[test]
fn ai_panel_renders() {
    let mut h = harness(SALES_CSV);
    key(&mut h, Key::I, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(h.state().ai.open);
    h.run_steps(2);
    shot(&mut h, "08-ai-panel");
}
