use super::{DIRECTORY, Kind, catalog::Arguments};

#[derive(Clone, Debug)]
pub enum Command {
    Message(String),
    Builtin { kind: Kind, arguments: String },
    Skill { selector: String, arguments: String },
}

/// Split once, without shell quoting, expansion or interpolation. The remainder
/// belongs to the user verbatim, including quotes, dollar signs and newlines.
pub(crate) fn split(text: &str) -> (&str, &str) {
    text.split_once(char::is_whitespace)
        .map(|(head, tail)| (head, tail.trim_start()))
        .unwrap_or((text, ""))
}

pub fn parse(text: &str) -> anyhow::Result<Command> {
    let text = text.trim_start();
    anyhow::ensure!(!text.trim().is_empty(), "输入不能为空");
    if !text.starts_with('/') {
        return Ok(Command::Message(text.trim().into()));
    }
    let (name, arguments) = split(text);
    let Some(entry) = DIRECTORY.iter().find(|entry| entry.name == name) else {
        let selector = &name[1..];
        anyhow::ensure!(
            !selector.is_empty()
                && selector
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
            "未知命令；输入 /help 查看用法"
        );
        return Ok(Command::Skill {
            selector: selector.into(),
            arguments: arguments.into(),
        });
    };
    let words: Vec<_> = arguments.split_whitespace().collect();
    let valid = match entry.rule {
        Arguments::None => words.is_empty(),
        Arguments::Optional => true,
        Arguments::Required => !arguments.is_empty(),
        Arguments::One => words.len() == 1,
        Arguments::Answer => words.len() >= 2,
        Arguments::Budget => {
            words.len() <= 2
                && words
                    .iter()
                    .all(|word| word.parse::<u64>().is_ok_and(|value| value > 0))
        }
        Arguments::Skills => {
            words.is_empty() || (words.first() == Some(&"run") && words.len() >= 2)
        }
        Arguments::Mcp => {
            words.is_empty()
                || (words.len() == 2
                    && matches!(words[0], "inspect" | "reconnect" | "enable" | "disable"))
        }
        Arguments::Config => words.is_empty() || crate::config::defaults::KEYS.contains(&words[0]),
    };
    anyhow::ensure!(valid, "参数有误：{}", entry.usage);
    Ok(Command::Builtin {
        kind: entry.kind,
        arguments: arguments.into(),
    })
}
