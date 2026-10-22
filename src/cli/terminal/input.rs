use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub struct Input {
    text: String,
    revision: u64,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    draft: String,
}

impl Input {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn insert(&mut self, text: &str) {
        // Pasting cannot inject terminal escapes or execute a slash command by itself.
        let text: String = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect();
        if self.text.len().saturating_add(text.len()) > 256 * 1024 {
            return;
        }
        if !text.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|index| *index >= self.cursor)
            .unwrap_or(self.text.len());
    }
    pub fn left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }
    pub fn right(&mut self) {
        self.cursor += self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map(str::len)
            .unwrap_or(0);
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        if self.cursor != end {
            self.revision = self.revision.wrapping_add(1);
        }
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        if start != self.cursor {
            self.revision = self.revision.wrapping_add(1);
        }
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }
    pub fn take(&mut self) -> String {
        self.revision = self.revision.wrapping_add(1);
        self.cursor = 0;
        self.history_index = None;
        self.draft.clear();
        std::mem::take(&mut self.text)
    }
    pub fn remember(&mut self, text: &str) {
        if !text.trim().is_empty() && self.history.last().is_none_or(|last| last != text) {
            self.history.push(text.to_owned());
            if self.history.len() > 200 {
                self.history.remove(0);
            }
        }
    }
    pub fn replace(&mut self, text: &str) {
        self.revision = self.revision.wrapping_add(1);
        self.text.clear();
        self.cursor = 0;
        self.insert(text);
    }
    /// Within multiline drafts arrows move between lines; at the boundary they
    /// browse submitted messages, retaining the unsent draft when moving back down.
    pub fn up(&mut self) {
        let start = self.text[..self.cursor].rfind('\n').map_or(0, |n| n + 1);
        if start > 0 {
            let column = self.text[start..self.cursor].graphemes(true).count();
            let previous = self.text[..start - 1].rfind('\n').map_or(0, |n| n + 1);
            self.cursor = previous
                + self.text[previous..start - 1]
                    .graphemes(true)
                    .take(column)
                    .map(str::len)
                    .sum::<usize>();
            return;
        }
        if self.history.is_empty() {
            return;
        }
        let index = match self.history_index {
            None => {
                self.draft = self.text.clone();
                self.history.len() - 1
            }
            Some(index) => index.saturating_sub(1),
        };
        self.history_index = Some(index);
        self.replace(&self.history[index].clone());
    }
    pub fn down(&mut self) {
        if let Some(end) = self.text[self.cursor..].find('\n') {
            let start = self.text[..self.cursor].rfind('\n').map_or(0, |n| n + 1);
            let column = self.text[start..self.cursor].graphemes(true).count();
            let next = self.cursor + end + 1;
            let end = self.text[next..]
                .find('\n')
                .map_or(self.text.len(), |n| next + n);
            self.cursor = next
                + self.text[next..end]
                    .graphemes(true)
                    .take(column)
                    .map(str::len)
                    .sum::<usize>();
            return;
        }
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 < self.history.len() {
            self.history_index = Some(index + 1);
            self.replace(&self.history[index + 1].clone());
        } else {
            self.history_index = None;
            let draft = std::mem::take(&mut self.draft);
            self.replace(&draft);
        }
    }
    pub fn position(&self, width: usize) -> (usize, usize) {
        let width = width.max(1);
        let (mut x, mut y) = (0, 0);
        let mut wrapped = false;
        for symbol in self.text[..self.cursor].graphemes(true) {
            if symbol == "\n" {
                x = 0;
                y += usize::from(!wrapped);
                wrapped = false;
                continue;
            }
            wrapped = false;
            let size = display_width(symbol).min(width);
            if x + size > width {
                x = 0;
                y += 1;
            }
            x += size;
            if x == width {
                x = 0;
                y += 1;
                wrapped = true;
            }
        }
        (x, y)
    }
}

fn display_width(symbol: &str) -> usize {
    if symbol == "\t" { 4 } else { symbol.width() }
}

pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![String::new()];
    let mut column = 0;
    let mut wrapped = false;
    for symbol in text.graphemes(true) {
        if symbol == "\n" {
            if !wrapped {
                lines.push(String::new());
            }
            column = 0;
            wrapped = false;
            continue;
        }
        wrapped = false;
        let size = display_width(symbol).min(width);
        if column + size > width {
            lines.push(String::new());
            column = 0;
        }
        lines
            .last_mut()
            .unwrap()
            .push_str(if symbol == "\t" { "    " } else { symbol });
        column += size;
        if column == width {
            lines.push(String::new());
            column = 0;
            wrapped = true;
        }
    }
    lines
}
