//! Editing follows grapheme boundaries and the same cells used by the renderer.
use super::{Input, display_width};
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn positions(
    text: &str,
    width: usize,
) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
    let width = width.max(1);
    let mut symbols = text.grapheme_indices(true);
    let (mut x, mut y, mut wrapped, mut finished) = (0, 0, false, false);
    std::iter::from_fn(move || {
        if finished {
            return None;
        }
        let Some((index, symbol)) = symbols.next() else {
            finished = true;
            return Some((text.len(), x, y));
        };
        let size = display_width(symbol).min(width);
        // A wide grapheme may wrap BEFORE it is drawn. Its leading cursor
        // belongs at column zero on the new row, not after the grapheme.
        if symbol != "\n" && x + size > width {
            x = 0;
            y += 1;
        }
        let position = (index, x, y);
        if symbol == "\n" {
            x = 0;
            y += usize::from(!wrapped);
            wrapped = false;
        } else {
            wrapped = false;
            x += size;
            if x == width {
                x = 0;
                y += 1;
                wrapped = true;
            }
        }
        Some(position)
    })
}

impl Input {
    pub fn visual_up(&mut self, width: usize) {
        let (_, row) = self.position(width);
        if row == 0 {
            self.up();
        } else {
            self.move_row(width, row - 1);
        }
    }

    pub fn visual_down(&mut self, width: usize) {
        let (_, row) = self.position(width);
        if !self.move_row(width, row + 1) {
            self.down();
        }
    }

    fn move_row(&mut self, width: usize, row: usize) -> bool {
        let (column, _) = self.position(width);
        let mut target = None;
        for (index, x, y) in positions(&self.text, width) {
            if y == row && (target.is_none() || x <= column) {
                target = Some(index);
            }
            if y > row {
                break;
            }
        }
        if let Some(target) = target {
            self.cursor = target;
            true
        } else {
            false
        }
    }

    pub fn line_home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |n| n + 1);
    }

    pub fn line_end(&mut self) {
        self.cursor += self.text[self.cursor..]
            .find('\n')
            .unwrap_or(self.text.len() - self.cursor);
    }

    pub fn word_left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .split_word_bound_indices()
            .rev()
            .find(|(_, part)| !part.trim().is_empty())
            .map_or(0, |(index, _)| index);
    }

    pub fn word_right(&mut self) {
        self.cursor += self.text[self.cursor..]
            .split_word_bound_indices()
            .find(|(_, part)| !part.trim().is_empty())
            .map_or(self.text.len() - self.cursor, |(index, part)| {
                index + part.len()
            });
    }

    pub fn delete_word(&mut self) {
        let end = self.cursor;
        self.word_left();
        self.remove(self.cursor, end);
    }

    pub fn kill_line(&mut self) {
        let start = self.cursor;
        self.line_end();
        let end = if self.cursor == start && start < self.text.len() {
            start + 1 // Delete the newline when already at the end of a line.
        } else {
            self.cursor
        };
        self.remove(start, end);
    }

    fn remove(&mut self, start: usize, end: usize) {
        if start != end {
            self.text.replace_range(start..end, "");
            self.revision = self.revision.wrapping_add(1);
        }
        self.cursor = start;
        self.align_cursor();
    }
}
