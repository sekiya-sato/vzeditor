//! 通常ファイルと巨大ファイルの Undo / Redo。

use super::{TextBuffer, UndoSnapshot, MAX_UNDO_HISTORY};

impl TextBuffer {
    pub(super) fn current_snapshot(&self) -> UndoSnapshot {
        UndoSnapshot {
            lines: self.lines.clone(),
            cursor_line: self.cursor_line,
            cursor_col: self.cursor_col,
            dirty: self.dirty,
        }
    }

    pub(super) fn push_undo_snapshot(&mut self) {
        self.push_undo(self.current_snapshot());
    }

    /// アンドゥ履歴に積む（上限を超えたら古いものから捨てる）。リドゥ履歴は消す。
    pub(super) fn push_undo(&mut self, snap: UndoSnapshot) {
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(snap);
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        match &self.huge {
            Some(h) => h.can_undo(),
            None => !self.undo_stack.is_empty(),
        }
    }

    pub fn can_redo(&self) -> bool {
        match &self.huge {
            Some(h) => h.can_redo(),
            None => !self.redo_stack.is_empty(),
        }
    }

    /// 巨大ファイルの Undo / Redo の後: 変更の位置へカーソルを置く。
    fn after_huge_history(&mut self, pos: Option<u64>) -> bool {
        let Some(h) = self.huge.as_ref() else {
            return false;
        };
        let Some(pos) = pos else {
            return false;
        };
        let (line, col) = h.offset_to_pos(pos).unwrap_or((0, 0));
        self.dirty = h.is_modified();
        self.anchor = None;
        self.typing_at = None;
        self.set_cursor(line, col);
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn undo(&mut self) -> bool {
        if let Some(h) = self.huge.as_mut() {
            let pos = h.undo();
            return self.after_huge_history(pos);
        }
        let Some(prev) = self.undo_stack.pop_back() else {
            return false;
        };
        self.redo_stack.push(self.current_snapshot());
        self.restore_snapshot(prev);
        true
    }

    pub fn redo(&mut self) -> bool {
        if let Some(h) = self.huge.as_mut() {
            let pos = h.redo();
            return self.after_huge_history(pos);
        }
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push_back(self.current_snapshot());
        self.restore_snapshot(next);
        true
    }

    fn restore_snapshot(&mut self, snap: UndoSnapshot) {
        self.anchor = None;
        self.lines = snap.lines;
        self.cursor_line = snap.cursor_line;
        self.cursor_col = snap.cursor_col;
        self.dirty = snap.dirty;
        self.preferred_visual_col = None;
        self.revision = self.revision.wrapping_add(1);
    }
}
