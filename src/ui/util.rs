//! Small UI helpers shared across the app.

use egui::{Context, Id, Key, KeyboardShortcut, Modifiers};

use crate::model;

pub(crate) fn rand_serial() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

pub fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string()
}

pub(crate) fn sc_text(ctx: &Context, key: Key, shift: bool) -> String {
    // egui's bundled fonts lack the ⇧ glyph, so spell Shift out.
    let base = ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key));
    if shift { format!("Shift+{base}") } else { base }
}

pub fn set_cursor_end(ctx: &Context, id: Id, text: &str) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let end = egui::text::CCursor::new(text.chars().count());
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(end)));
        state.store(ctx, id);
    }
}

/// Add missing closing parentheses, like Excel does on commit.
pub(crate) fn autoclose_parens(s: &str) -> String {
    if !model::is_formula(s) {
        return s.to_string();
    }
    let mut depth = 0i32;
    let mut in_str = false;
    for c in s.chars() {
        match c {
            '"' => in_str = !in_str,
            '(' if !in_str => depth += 1,
            ')' if !in_str => depth -= 1,
            _ => {}
        }
    }
    if depth > 0 && !in_str {
        format!("{s}{}", ")".repeat(depth as usize))
    } else {
        s.to_string()
    }
}
