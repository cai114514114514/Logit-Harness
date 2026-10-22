//! The secret editor is separate from chat input, history and durable events.
use super::{Input, Screen};
use crate::config::{Config, Paths, api};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

mod view;
pub(super) use view::render;

pub(crate) struct Form {
    endpoint: Input,
    secret: Input,
    field: usize,
    configured: bool,
    environment: bool,
    error: Option<String>,
}

pub(super) fn open(screen: &mut Screen, paths: &Paths) {
    match Form::load(paths) {
        Ok(form) => {
            screen.setup = Some(form);
            screen.menu_hidden = true;
        }
        Err(error) => screen.note(format!("无法打开 API 设置：{error}")),
    }
}

pub(super) fn reject_inline(screen: &mut Screen, text: &str) -> bool {
    let mut words = text.split_whitespace();
    if words.next() == Some("/setup") && words.next().is_some() {
        screen.input.take();
        screen.note("请只输入 /setup，在隐藏输入框中填写密钥；不接受命令参数。");
        return true;
    }
    false
}

impl Form {
    fn load(paths: &Paths) -> anyhow::Result<Self> {
        let config = Config::load(&paths.home)?;
        let mut endpoint = Input::default();
        endpoint.insert(&config.base_url);
        Ok(Self {
            endpoint,
            secret: Input::default(),
            field: 0,
            configured: api::configured(paths, &config.base_url)?,
            environment: api::source(paths, &config.base_url)? == "environment",
            error: None,
        })
    }
    fn input(&mut self) -> &mut Input {
        if self.field == 0 {
            &mut self.endpoint
        } else {
            &mut self.secret
        }
    }
    fn select(&mut self, field: usize, paths: &Paths) {
        self.field = field;
        self.error = None;
        if field == 1 {
            // An existing key only applies to its own endpoint. Recheck after
            // editing the URL so the placeholder never promises reuse elsewhere.
            self.configured = api::configured(paths, self.endpoint.text()).unwrap_or(false);
        }
    }
    fn save(&self, paths: &Paths) -> anyhow::Result<()> {
        let key = self.secret.text().trim();
        api::save(
            paths,
            self.endpoint.text(),
            (!key.is_empty()).then_some(key),
        )
    }
}

/// Returns true while the settings panel owns input. Nothing typed here is
/// passed to chat, even when a key is pasted or a save fails.
pub(super) fn handle(screen: &mut Screen, event: &Event, paths: &Paths) -> bool {
    let Some(form) = screen.setup.as_mut() else {
        return false;
    };
    match event {
        Event::Paste(text) => {
            let text = text.trim();
            if text.contains(['\n', '\r']) {
                form.error = Some("请粘贴单行地址或 API Key".into());
            } else {
                form.input().insert(text);
            }
        }
        Event::Key(key) if key.kind != KeyEventKind::Release => {
            let control = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Esc => {
                    screen.setup = None;
                    screen.menu_hidden = false;
                }
                KeyCode::Char('c') if control => {
                    screen.setup = None;
                    screen.menu_hidden = false;
                    // In a live session Ctrl-C still reaches the pause handler.
                    return screen.session.is_none();
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                    form.select(1 - form.field, paths);
                }
                KeyCode::Enter if form.field == 0 => form.select(1, paths),
                KeyCode::Enter => match form.save(paths) {
                    Ok(()) => {
                        let environment = form.environment;
                        screen.setup = None;
                        screen.menu_hidden = false;
                        screen.key_present = true;
                        screen.note(if environment {
                            "API 设置已保存（未联网验证）。当前 DEEPSEEK_API_KEY 环境变量仍优先；取消该变量后使用保存的密钥。"
                        } else if screen.session.is_some() {
                            "API 设置已保存（未联网验证）。用于新任务；已运行的后台会话在重启后读取新配置。"
                        } else { "API 设置已保存（未联网验证）。现在可以直接输入任务。" });
                    }
                    Err(error) => form.error = Some(error.to_string()),
                },
                KeyCode::Char('u') if control => {
                    form.input().take();
                }
                KeyCode::Char('a') if control => form.input().home(),
                KeyCode::Char('e') if control => form.input().end(),
                KeyCode::Char(character) if !control => form.input().insert(&character.to_string()),
                KeyCode::Backspace => form.input().backspace(),
                KeyCode::Delete => form.input().delete(),
                KeyCode::Left => form.input().left(),
                KeyCode::Right => form.input().right(),
                KeyCode::Home => form.input().home(),
                KeyCode::End => form.input().end(),
                _ => (),
            }
        }
        _ => (),
    }
    true
}
