//! Several workbooks open at once, as tabs.

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use super::*;

fn write_csv(name: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "litesheet-docs-{}-{}",
        std::process::id(),
        rand_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

fn open(h: &mut Harness<'_, App>, path: &std::path::Path) {
    let ctx = h.ctx.clone();
    h.state_mut().open_path(path.to_path_buf(), &ctx);
    for _ in 0..200 {
        h.step();
        let app = h.state();
        if app.loading.is_none() && app.docs.iter().all(|d| !d.is_loading()) {
            h.run_steps(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("load timed out");
}

#[test]
fn opening_a_second_file_keeps_the_first() {
    let mut h = harness(SALES_CSV);
    let other = write_csv("budget.csv", "Item,Cost\nRent,1200\nFood,300\n");
    open(&mut h, &other);
    assert_eq!(h.state().docs.len(), 2);
    assert_eq!(display(&h, 1, 0), "Rent");
    // Both tabs are shown; clicking the first brings its data back.
    h.get_by_label("budget.csv");
    h.get_by_label("sales.csv").click();
    h.run_steps(2);
    assert_eq!(h.state().current, 0);
    assert_eq!(display(&h, 1, 2), "West");
    shot(&mut h, "workbook-tabs");

    // Opening a file that is already open switches to it instead.
    open(&mut h, &other);
    assert_eq!(h.state().docs.len(), 2);
    assert_eq!(display(&h, 1, 0), "Rent");

    // ⌃Tab cycles through the tabs.
    key(&mut h, Key::Tab, Modifiers::CTRL);
    assert_eq!(display(&h, 1, 2), "West");

    // ⌘W closes the active tab; the other one stays.
    key(&mut h, Key::W, Modifiers::COMMAND);
    assert_eq!(h.state().docs.len(), 1);
    assert_eq!(display(&h, 1, 0), "Rent");
}

#[test]
fn new_workbooks_open_in_tabs_and_a_blank_one_is_reused() {
    let mut h = harness(SALES_CSV);
    key(&mut h, Key::N, Modifiers::COMMAND);
    assert_eq!(h.state().docs.len(), 2);
    assert_eq!(h.state().current, 1);
    assert_eq!(display(&h, 1, 2), "");
    // A file opened from the untouched blank tab replaces it.
    let other = write_csv("budget.csv", "Item,Cost\nRent,1200\n");
    open(&mut h, &other);
    assert_eq!(h.state().docs.len(), 2);
    assert_eq!(display(&h, 1, 0), "Rent");
}

#[test]
fn quitting_asks_about_each_unsaved_workbook() {
    let mut h = harness(SALES_CSV);
    h.state_mut().book.set_input(0, (1, 0), "changed");
    let other = write_csv("budget.csv", "Item,Cost\nRent,1200\n");
    open(&mut h, &other);
    assert_eq!(h.state().current, 1);

    let ctx = h.ctx.clone();
    h.state_mut().request_quit(&ctx);
    h.run_steps(2);
    // It switches to the unsaved workbook and asks.
    assert_eq!(h.state().current, 0);
    h.get_by_label("Save changes to \"sales.csv\"?");
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().pending.is_none() && !h.state().allow_close);

    h.state_mut().request_quit(&ctx);
    h.run_steps(2);
    h.get_by_label("Don't Save").click();
    h.run_steps(2);
    assert!(h.state().allow_close);
}

#[test]
fn copy_and_paste_between_workbooks() {
    let mut h = harness(SALES_CSV);
    h.state_mut().sel = crate::ui::Selection {
        anchor: (1, 2),
        cursor: (2, 3),
    };
    let ctx = h.ctx.clone();
    h.state_mut().copy(&ctx, true);
    let text = h.state().clip.as_ref().unwrap().text.clone();
    let other = write_csv("budget.csv", "Item,Cost\n");
    open(&mut h, &other);
    h.state_mut().sel = crate::ui::Selection::single((5, 0));
    h.state_mut().paste(&text);
    assert_eq!(display(&h, 5, 0), "West");
    assert_eq!(display(&h, 6, 1), "Gadget");
    // A cut from another workbook doesn't empty the source.
    h.state_mut().switch_doc(0);
    assert_eq!(display(&h, 1, 2), "West");
}
