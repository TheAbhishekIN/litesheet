//! Cell editing: starting, committing and cancelling edits, and building
//! formula references by clicking cells ("point mode").

use egui::{Context, Id};

use super::app::{App, BAR_EDITOR_ID, CELL_EDITOR_ID, EditState};
use super::selection::Selection;
use super::util::autoclose_parens;
use crate::model::Pos;

impl App {
    pub(crate) fn start_edit(&mut self, ctx: &Context, initial: Option<String>, in_bar: bool) {
        let pos = self.sel.cursor;
        let buf = initial.unwrap_or_else(|| self.sheet().edit_text(pos));
        self.edit = Some(EditState {
            pos,
            buf,
            in_bar,
            place_cursor: true,
            pointing: None,
        });
        let id = Id::new(if in_bar {
            BAR_EDITOR_ID
        } else {
            CELL_EDITOR_ID
        });
        ctx.memory_mut(|m| m.request_focus(id));
    }

    /// Commit the edit, then move the cursor by `mv`.
    pub(crate) fn commit_edit(&mut self, mv: (i64, i64)) {
        let Some(edit) = self.edit.take() else { return };
        let old = self.sheet().edit_text(edit.pos);
        if edit.buf != old {
            let buf = autoclose_parens(&edit.buf);
            self.book.set_input(self.active, edit.pos, &buf);
            self.data_changed();
        }
        self.sel = Selection::single(edit.pos);
        if mv != (0, 0) {
            self.move_cursor(mv.0, mv.1, false);
        }
    }

    pub(crate) fn cancel_edit(&mut self) {
        self.edit = None;
    }

    /// Whether clicking a cell right now should insert a reference into the formula.
    pub(crate) fn wants_reference(&self) -> bool {
        match &self.edit {
            Some(e) if e.buf.starts_with('=') => {
                if e.pointing.is_some() {
                    return true;
                }
                matches!(
                    e.buf.trim_end().chars().last(),
                    Some(
                        '=' | '(' | ',' | '+' | '-' | '*' | '/' | '^' | '&' | ':' | '<' | '>' | ';'
                    )
                )
            }
            _ => false,
        }
    }

    pub(crate) fn point_at(&mut self, pos: Pos, start: bool) {
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        if start || edit.pointing.is_none() {
            edit.pointing = Some((pos, edit.buf.len()));
        }
        let (anchor, len) = edit.pointing.unwrap();
        edit.buf.truncate(len);
        let s = Selection {
            anchor,
            cursor: pos,
        };
        edit.buf.push_str(&s.name());
        edit.place_cursor = true;
    }
}
