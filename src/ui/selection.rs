//! A rectangular cell selection with an anchor and a moving cursor.

use crate::model::{Pos, refs};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub cursor: Pos,
}

impl Selection {
    pub fn single(pos: Pos) -> Self {
        Self {
            anchor: pos,
            cursor: pos,
        }
    }
    /// (top-left, bottom-right), inclusive.
    pub fn range(&self) -> (Pos, Pos) {
        (
            (
                self.anchor.0.min(self.cursor.0),
                self.anchor.1.min(self.cursor.1),
            ),
            (
                self.anchor.0.max(self.cursor.0),
                self.anchor.1.max(self.cursor.1),
            ),
        )
    }
    pub fn contains(&self, pos: Pos) -> bool {
        let ((r0, c0), (r1, c1)) = self.range();
        (r0..=r1).contains(&pos.0) && (c0..=c1).contains(&pos.1)
    }
    pub fn is_single(&self) -> bool {
        self.anchor == self.cursor
    }
    pub fn name(&self) -> String {
        let (a, b) = self.range();
        if a == b {
            refs::cell_name(a.0, a.1)
        } else {
            format!(
                "{}:{}",
                refs::cell_name(a.0, a.1),
                refs::cell_name(b.0, b.1)
            )
        }
    }
}
