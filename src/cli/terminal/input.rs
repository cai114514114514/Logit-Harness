use crate::runtime::state::Control;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub struct Input {
    text: String,
    cursor: usize,
}

impl Input {
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
            .last()
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
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
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

pub enum Intent {
    Control(Control),
    Status,
    Budget,
    Diff,
    Help,
    Detach,
}

pub fn parse(text: &str) -> anyhow::Result<Intent> {
    let text = text.trim();
    anyhow::ensure!(!text.is_empty(), "输入不能为空");
    if !text.starts_with('/') {
        return Ok(Intent::Control(Control::Message { text: text.into() }));
    }
    let parts: Vec<_> = text.split_whitespace().collect();
    Ok(match parts[0] {
        "/pause" if parts.len() == 1 => Intent::Control(Control::Pause),
        "/resume" if parts.len() == 1 => Intent::Control(Control::Resume),
        "/stop" if parts.len() == 1 => Intent::Control(Control::Stop),
        "/status" if parts.len() == 1 => Intent::Status,
        "/diff" if parts.len() == 1 => Intent::Diff,
        "/help" if parts.len() == 1 => Intent::Help,
        "/detach" if parts.len() == 1 => Intent::Detach,
        "/budget" if parts.len() == 1 => Intent::Budget,
        "/budget" if (2..=3).contains(&parts.len()) => {
            let tokens: u64 = parts[1].parse()?;
            let seconds: Option<u64> = parts.get(2).map(|value| value.parse()).transpose()?;
            anyhow::ensure!(
                tokens > 0 && seconds.is_none_or(|value| value > 0),
                "预算上限必须为正数"
            );
            Intent::Control(Control::Budget {
                tokens: Some(tokens),
                seconds,
            })
        }
        "/approve" if parts.len() == 2 => Intent::Control(Control::Approve {
            id: parts[1].into(),
        }),
        "/answer" if parts.len() >= 3 => {
            let answer = text
                .strip_prefix("/answer")
                .unwrap()
                .trim_start()
                .strip_prefix(parts[1])
                .unwrap()
                .trim_start();
            Intent::Control(Control::Answer {
                id: parts[1].into(),
                text: answer.into(),
            })
        }
        _ => anyhow::bail!("未知命令或参数有误；输入 /help 查看用法"),
    })
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
