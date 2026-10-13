//! Keyboard shortcuts and grid navigation.

use egui::{Context, Id, Key, KeyboardShortcut, Modifiers};

use super::app::{App, BAR_EDITOR_ID, CELL_EDITOR_ID, Pending};
use super::selection::Selection;

impl App {
    pub(crate) fn shortcuts(&mut self, ctx: &Context) {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let sc = |m, k| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));

        if sc(cmd, Key::N) {
            self.request(Pending::New, ctx);
        }
        if sc(cmd, Key::O) {
            self.request(Pending::Open, ctx);
        }
        if sc(cmd_shift, Key::S) {
            self.save(true);
        }
        if sc(cmd, Key::S) {
            self.save(false);
        }
        if sc(cmd, Key::F) {
            self.commit_edit((0, 0));
            self.show_search = true;
            self.focus_search = true;
        }
        if sc(cmd_shift, Key::L) {
            self.filter_buttons = !self.filter_buttons;
        }
        if sc(cmd_shift, Key::I) {
            self.ai.open = !self.ai.open;
        }

        // Keys while editing a cell.
        if self.edit.is_some() {
            let (cell_id, bar_id) = (Id::new(CELL_EDITOR_ID), Id::new(BAR_EDITOR_ID));
            let focused = ctx.memory(|m| m.focused());
            // Another text field (search, AI prompt, …) owns the keyboard.
            if focused.is_some_and(|f| f != cell_id && f != bar_id) {
                return;
            }
            if sc(Modifiers::SHIFT, Key::Enter) {
                self.commit_edit((-1, 0));
            } else if sc(Modifiers::NONE, Key::Enter) {
                self.commit_edit((1, 0));
            } else if sc(Modifiers::SHIFT, Key::Tab) {
                self.commit_edit((0, -1));
            } else if sc(Modifiers::NONE, Key::Tab) {
                self.commit_edit((0, 1));
            } else if sc(Modifiers::NONE, Key::Escape) {
                self.cancel_edit();
            } else {
                // Focus was dropped (egui clears it on Escape or a click on the grid):
                // give it back so typing continues in the editor.
                if focused.is_none() {
                    let in_bar = self.edit.as_ref().is_some_and(|e| e.in_bar);
                    ctx.memory_mut(|m| m.request_focus(if in_bar { bar_id } else { cell_id }));
                }
                return;
            }
            ctx.memory_mut(|m| m.stop_text_input());
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        if sc(cmd_shift, Key::Z) || sc(cmd, Key::Y) {
            self.redo();
        }
        if sc(cmd, Key::Z) {
            self.undo();
        }
        if sc(cmd, Key::B) {
            self.toggle_bold();
        }
        if sc(cmd, Key::I) {
            self.toggle_italic();
        }
        if sc(cmd, Key::U) {
            self.toggle_underline();
        }
        if sc(cmd, Key::D) {
            self.fill(true);
        }
        if sc(cmd, Key::R) {
            self.fill(false);
        }
        if sc(cmd, Key::A) {
            let s = self.sheet();
            self.sel = Selection {
                anchor: (0, 0),
                cursor: (s.n_rows.max(1) - 1, s.n_cols.max(1) - 1),
            };
        }

        // Navigation.
        let page = 25;
        for (key, dr, dc) in [
            (Key::ArrowUp, -1i64, 0i64),
            (Key::ArrowDown, 1, 0),
            (Key::ArrowLeft, 0, -1),
            (Key::ArrowRight, 0, 1),
            (Key::PageUp, -page, 0),
            (Key::PageDown, page, 0),
        ] {
            if sc(cmd_shift, key) || sc(cmd, key) {
                let extend = ctx.input(|i| i.modifiers.shift);
                let (r, c) = self.sel.cursor;
                let s = self.sheet();
                let target = match key {
                    Key::ArrowUp => (self.row_at(0).unwrap_or(0), c),
                    Key::ArrowDown => (s.n_rows.saturating_sub(1), c),
                    Key::ArrowLeft => (r, 0),
                    _ => (r, s.n_cols.saturating_sub(1)),
                };
                self.set_cursor(target, extend);
            } else if sc(Modifiers::SHIFT, key) {
                self.move_cursor(dr, dc, true);
            } else if sc(Modifiers::NONE, key) {
                self.move_cursor(dr, dc, false);
            }
        }
        if sc(Modifiers::NONE, Key::Home) {
            self.set_cursor((self.sel.cursor.0, 0), false);
        }
        if sc(Modifiers::SHIFT, Key::Tab) {
            self.move_cursor(0, -1, false);
        }
        if sc(Modifiers::NONE, Key::Tab) {
            self.move_cursor(0, 1, false);
        }
        if sc(Modifiers::SHIFT, Key::Enter) {
            self.move_cursor(-1, 0, false);
        }
        if sc(Modifiers::NONE, Key::Enter) || sc(Modifiers::NONE, Key::F2) {
            self.start_edit(ctx, None, false);
            return;
        }
        if sc(Modifiers::NONE, Key::Delete) || sc(Modifiers::NONE, Key::Backspace) {
            if self.selected_chart.is_some() {
                self.delete_selected_chart();
            } else {
                self.clear_selection();
            }
        }
        // Esc deselects a chart first, then drops the copy outline and multi-selection.
        if sc(Modifiers::NONE, Key::Escape) && self.selected_chart.take().is_none() {
            self.clip = None;
            self.sel = Selection::single(self.sel.cursor);
        }

        // Clipboard and typing.
        // These events are consumed so the cell editor (focused below) doesn't see them twice.
        let events = ctx.input_mut(|i| {
            let (ours, rest) = std::mem::take(&mut i.events).into_iter().partition(|e| {
                matches!(
                    e,
                    egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::Paste(_)
                        | egui::Event::Text(_)
                )
            });
            i.events = rest;
            ours
        });
        for ev in events {
            match ev {
                egui::Event::Copy => self.copy(ctx, false),
                egui::Event::Cut => self.copy(ctx, true),
                egui::Event::Paste(text) => self.paste(&text),
                egui::Event::Text(t) if !t.is_empty() => match self.edit.as_mut() {
                    Some(edit) => edit.buf.push_str(&t),
                    None => self.start_edit(ctx, Some(t), false),
                },
                _ => {}
            }
        }
    }
}
