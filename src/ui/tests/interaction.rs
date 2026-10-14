//! Mouse and keyboard interaction: selection, editing, clipboard, sort,
//! filter and search.

use egui::{Event, Key, Modifiers, pos2};
use egui_kittest::kittest::Queryable;

use super::*;

#[test]
fn renders_initial() {
    let mut h = harness(SALES_CSV);
    // The support button and Help menu are in the menu bar.
    h.get_by_label_contains("Buy me a coffee");
    h.get_by_label("Help");
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
    click(&mut h, pos2(50.0 + 300.0 - 9.0, HEADER_Y), Modifiers::NONE);
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

fn right_click(h: &mut Harness<'_, App>, p: egui::Pos2) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

#[test]
fn insert_and_delete_rows_from_context_menu() {
    let mut h = harness(SALES_CSV);
    click(&mut h, cell(9, 5), Modifiers::NONE);
    type_text(&mut h, "=SUM(F2:F9)");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(display(&h, 9, 5), "346.23");

    // Right-click the header of row 4 and insert a row above it.
    right_click(&mut h, pos2(25.0, cell(3, 0).y));
    assert_eq!(h.state().sel.name(), "A4:G4");
    shot(&mut h, "row-context-menu");
    h.get_by_label("Insert Row Above").click();
    h.run_steps(2);
    assert_eq!(display(&h, 3, 0), "");
    assert_eq!(display(&h, 4, 0), "1003");
    assert_eq!(h.state().sheet().cells[&(10, 5)].raw, "=SUM(F2:F10)");
    assert_eq!(display(&h, 10, 5), "346.23");

    // Delete it again from the column-and-row menu on a cell.
    right_click(&mut h, cell(3, 2));
    h.get_by_label("Delete Row").click();
    h.run_steps(2);
    assert_eq!(display(&h, 3, 0), "1003");
    assert_eq!(h.state().sheet().cells[&(9, 5)].raw, "=SUM(F2:F9)");

    // Insert a column left of B from its header; Date moves to C.
    right_click(&mut h, pos2(200.0, HEADER_Y));
    assert_eq!(h.state().sel.name(), "B1:B10");
    h.get_by_label("Insert Column Left").click();
    h.run_steps(2);
    assert_eq!(display(&h, 0, 2), "Date");
    assert_eq!(h.state().sheet().cells[&(9, 6)].raw, "=SUM(G2:G9)");
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert_eq!(display(&h, 0, 1), "Date");
}

#[test]
fn number_format_menu_and_decimals() {
    let mut h = harness(SALES_CSV);
    // Select the Price column values F2:F9 and make them dollars.
    click(&mut h, cell(1, 5), Modifiers::NONE);
    click(&mut h, cell(8, 5), Modifiers::SHIFT);
    h.get_by_label("Format").click();
    h.run_steps(2);
    h.get_by_label_contains("Currency").hover();
    h.run_steps(3);
    shot(&mut h, "format-menu");
    h.get_by_label_contains("Dollar").click();
    h.run_steps(2);
    assert_eq!(display(&h, 2, 5), "$120.00");
    assert_eq!(display(&h, 1, 5), "$19.99");

    h.state_mut().adjust_decimals(-1);
    h.run_steps(1);
    assert_eq!(display(&h, 1, 5), "$20.0");
    h.state_mut().adjust_decimals(-1);
    assert_eq!(display(&h, 2, 5), "$120");

    // Delete clears the values but keeps the format for the next entry.
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert_eq!(display(&h, 1, 5), "");
    click(&mut h, cell(1, 5), Modifiers::NONE);
    type_text(&mut h, "5");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(display(&h, 1, 5), "$5");

    // Editing a currency cell shows the plain number.
    assert_eq!(h.state().sheet().edit_text((1, 5)), "5");
}

#[test]
fn cell_styles_from_toolbar_and_shortcuts() {
    use crate::model::{Borders, HAlign, Rgb};
    let mut h = harness(SALES_CSV);
    let style = |h: &Harness<'_, App>, r, c| {
        h.state()
            .sheet()
            .cells
            .get(&(r, c))
            .map(|c| c.style)
            .unwrap_or_default()
    };

    // Header row: ⌘B, a fill, centred, with an outline.
    click(&mut h, cell(0, 0), Modifiers::NONE);
    click(&mut h, cell(0, 6), Modifiers::SHIFT);
    key(&mut h, Key::B, Modifiers::COMMAND);
    assert!(style(&h, 0, 3).bold);
    h.state_mut().set_fill(Some(Rgb(22, 163, 74)));
    h.get_by_label("Center").click();
    h.run_steps(2);
    assert_eq!(style(&h, 0, 6).align, HAlign::Center);
    h.state_mut()
        .set_borders(crate::ui::commands::BorderKind::Outline);
    assert_eq!(
        style(&h, 0, 0).borders,
        Borders {
            top: true,
            bottom: true,
            left: true,
            right: false
        }
    );
    assert_eq!(
        style(&h, 0, 6).borders,
        Borders {
            top: true,
            bottom: true,
            left: false,
            right: true
        }
    );

    // Prices in red italic via the toolbar's I button.
    click(&mut h, cell(1, 5), Modifiers::NONE);
    click(&mut h, cell(8, 5), Modifiers::SHIFT);
    h.get_by_label("I").click();
    h.run_steps(2);
    h.state_mut().set_text_color(Some(Rgb(192, 0, 0)));
    assert!(style(&h, 4, 5).italic);
    assert_eq!(style(&h, 4, 5).color, Some(Rgb(192, 0, 0)));
    shot(&mut h, "styles");

    // ⌘B again on a bold cell turns bold off for the selection.
    click(&mut h, cell(0, 0), Modifiers::NONE);
    key(&mut h, Key::B, Modifiers::COMMAND);
    assert!(!style(&h, 0, 0).bold);
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert!(style(&h, 0, 0).bold);

    // Typing over a styled cell keeps the style; Delete keeps it too.
    type_text(&mut h, "Order #");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(display(&h, 0, 0), "Order #");
    assert!(style(&h, 0, 0).bold);
    click(&mut h, cell(0, 0), Modifiers::NONE);
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert_eq!(display(&h, 0, 0), "");
    assert_eq!(style(&h, 0, 0).fill, Some(Rgb(22, 163, 74)));
    h.state_mut().clear_formats();
    assert!(!h.state().sheet().cells.contains_key(&(0, 0)));
}

#[test]
fn merge_cells_from_toolbar() {
    let mut h = harness(SALES_CSV);
    // Insert a title row, then merge it across A1:G1.
    click(&mut h, cell(0, 0), Modifiers::NONE);
    h.state_mut().insert_rows(false);
    click(&mut h, cell(0, 0), Modifiers::NONE);
    type_text(&mut h, "Sales orders");
    key(&mut h, Key::Enter, Modifiers::NONE);
    click(&mut h, cell(0, 0), Modifiers::NONE);
    click(&mut h, cell(0, 6), Modifiers::SHIFT);
    h.get_by_label("Merge & Center").click();
    h.run_steps(2);
    assert_eq!(h.state().sheet().merges, vec![((0, 0), (0, 6))]);

    // Clicking anywhere in the merge selects all of it, with A1 active.
    click(&mut h, cell(0, 4), Modifiers::NONE);
    assert_eq!(h.state().sel.cursor, (0, 0));
    assert_eq!(h.state().sel.name(), "A1:G1");
    assert!(h.state().sel_is_single_cell());
    shot(&mut h, "merged");

    // Arrows step over the merge; Up from below lands on its top-left.
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(h.state().sel.cursor, (0, 7));
    key(&mut h, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.state().sel.cursor, (0, 0));
    click(&mut h, cell(1, 3), Modifiers::NONE);
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(h.state().sel.cursor, (0, 0));

    // Editing a merged cell edits its top-left.
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.state_mut().edit.as_mut().unwrap().buf = "Orders 2024".into();
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(display(&h, 0, 0), "Orders 2024");

    // Sorting across merged rows is refused rather than scrambling data.
    click(&mut h, cell(0, 0), Modifiers::NONE);
    click(&mut h, cell(8, 0), Modifiers::SHIFT);
    h.state_mut().sort_by(4, true);
    assert_eq!(display(&h, 2, 0), "1001");

    // The same button unmerges.
    click(&mut h, cell(0, 0), Modifiers::NONE);
    h.get_by_label("Merge & Center").click();
    h.run_steps(2);
    assert!(h.state().sheet().merges.is_empty());
}

#[test]
fn freeze_panes_keep_headers_in_view() {
    use crate::model::Freeze;
    let mut h = harness(SALES_CSV);
    // Freeze at B2: row 1 and column A stay put.
    click(&mut h, cell(1, 1), Modifiers::NONE);
    h.get_by_label("View").click();
    h.run_steps(2);
    h.get_by_label("Freeze Panes at B2").click();
    h.run_steps(2);
    assert_eq!(h.state().sheet().freeze, Freeze { rows: 1, cols: 1 });

    // Scroll down 3 rows and right past column C.
    let mid = cell(6, 4);
    h.event(Event::PointerMoved(mid));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-210.0, -66.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    shot(&mut h, "frozen");

    // The frozen header row and column are still where they were…
    click(&mut h, cell(0, 0), Modifiers::NONE);
    assert_eq!(h.state().sel.cursor, (0, 0));
    click(&mut h, cell(4, 0), Modifiers::NONE);
    assert_eq!(
        h.state().sel.cursor,
        (7, 0),
        "column A is pinned, rows scrolled by 3"
    );
    click(&mut h, cell(0, 3), Modifiers::NONE);
    assert_eq!(h.state().sel.cursor.0, 0, "row 1 is pinned");
    assert!(h.state().sel.cursor.1 > 3, "columns scrolled");
    // …and a scrolled cell is under the pointer as expected.
    click(&mut h, cell(4, 4), Modifiers::NONE);
    assert_eq!(h.state().sel.cursor.0, 7);

    // Unfreeze, which is undoable.
    h.state_mut().set_freeze(Freeze::default());
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert_eq!(h.state().sheet().freeze, Freeze { rows: 1, cols: 1 });
}

#[test]
fn insert_move_and_delete_a_chart() {
    use crate::model::ChartKind;
    let mut h = harness(SALES_CSV);
    // Chart the Units and Price columns against Product.
    click(&mut h, cell(0, 3), Modifiers::NONE);
    click(&mut h, cell(8, 5), Modifiers::SHIFT);
    h.get_by_label("Insert Chart").click();
    h.run_steps(2);
    assert_eq!(h.state().chart_dialog.as_ref().unwrap().range, "D1:F9");
    h.get_by_label("Line").click();
    h.run_steps(2);
    shot(&mut h, "chart-dialog");
    h.get_by_label("Create Chart").click();
    h.run_steps(3);
    let chart = h.state().sheet().charts[0].clone();
    assert_eq!(chart.kind, ChartKind::Line);
    assert_eq!(chart.series.len(), 2);
    assert_eq!(chart.anchor, (0, 7), "placed to the right of the data");
    assert_eq!(h.state().selected_chart, Some(0));
    shot(&mut h, "chart-on-sheet");

    // Drag it down by four rows.
    let start = cell(2, 9);
    h.event(Event::PointerMoved(start));
    h.event(Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    for i in 1..=8 {
        h.event(Event::PointerMoved(
            start + egui::vec2(0.0, 11.0 * i as f32),
        ));
        h.step();
    }
    let end = start + egui::vec2(0.0, 88.0);
    h.event(Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    assert_eq!(h.state().sheet().charts[0].anchor, (4, 7));
    assert_eq!(h.state().book.undo_label(), Some("Move chart"));

    // The chart reads live values.
    click(&mut h, cell(1, 4), Modifiers::NONE);
    type_text(&mut h, "99");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let data = h.state().sheet().charts[0].data(&h.state().book);
    assert_eq!(data.series[0].1[0], Some(99.0));

    // Select it and press Delete; undo brings it back.
    click(&mut h, cell(8, 9), Modifiers::NONE);
    assert_eq!(h.state().selected_chart, Some(0));
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert!(h.state().sheet().charts.is_empty());
    assert_eq!(display(&h, 8, 9), "", "Delete removed the chart, not cells");
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert_eq!(h.state().sheet().charts.len(), 1);
}

#[test]
fn files_opened_from_finder_respect_unsaved_changes() {
    let dir = std::env::temp_dir().join(format!("litesheet-finder-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let other = dir.join("budget.csv");
    std::fs::write(&other, "Item,Cost\nRent,1200\n").unwrap();

    let mut h = harness(SALES_CSV);
    click(&mut h, cell(1, 1), Modifiers::NONE);
    type_text(&mut h, "edited");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().book.dirty);

    // macOS asks us to open another file: unsaved work triggers the prompt.
    crate::platform::simulate_open([other.clone()]);
    h.run_steps(3);
    h.get_by_label_contains("Save changes to"); // panics if the prompt is missing
    assert_eq!(display(&h, 1, 1), "edited", "nothing replaced yet");
    h.get_by_label("Don't Save").click();
    for _ in 0..50 {
        h.run_steps(2);
        if h.state().sheet().name == "budget" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(display(&h, 1, 1), "1200");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn loading_screen_and_cancel() {
    let mut h = harness(SALES_CSV);
    let path = std::path::PathBuf::from("/tmp/IPC_Master_Export.xlsx");
    let (loading, _tx) = crate::ui::files::Loading::stalled(path);
    h.state_mut().loading = Some(loading);
    h.run_steps(2);
    h.get_by_label("Opening IPC_Master_Export.xlsx");
    h.get_by_label("Reading sheet 1 of 2: Data…");
    h.get_by_label("9.1 MB");
    shot(&mut h, "loading");
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().loading.is_none());
    // The workbook that was open before is still there.
    assert_eq!(display(&h, 1, 2), "West");
}
