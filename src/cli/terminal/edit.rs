use super::Screen;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Shared editor for the welcome screen and a running session. Returning false
/// leaves submission/lifecycle keys to the caller that owns the connection.
pub(super) fn edit(screen: &mut Screen, key: KeyEvent) -> bool {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let menu = screen.menu();
    let choices = if let Some((entries, _)) = &screen.skill_choices {
        entries.len()
    } else if screen.picker {
        screen.recent.len()
    } else {
        menu.len()
    };
    match key.code {
        KeyCode::Up if choices > 0 => screen.selection = screen.selection.saturating_sub(1),
        KeyCode::Down if choices > 0 => screen.selection = (screen.selection + 1).min(choices - 1),
        KeyCode::Esc => {
            screen.picker = false;
            screen.skill_choices = None;
            screen.menu_hidden = true;
            screen.selection = 0;
        }
        KeyCode::Tab if !menu.is_empty() => {
            screen.complete();
        }
        KeyCode::Char('u') if control => {
            screen.input.take();
            screen.menu_hidden = false;
        }
        KeyCode::Char('a') if control => screen.input.home(),
        KeyCode::Char('e') if control => screen.input.end(),
        KeyCode::Char('d') if control && !screen.input.text().is_empty() => screen.input.delete(),
        KeyCode::Enter
            if key
                .modifiers
                .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
        {
            screen.input.insert("\n")
        }
        KeyCode::Char(character) if !control => {
            screen.input.insert(&character.to_string());
            screen.menu_hidden = false;
            screen.selection = 0;
        }
        KeyCode::Backspace => {
            screen.input.backspace();
            screen.menu_hidden = false;
            screen.selection = 0;
        }
        KeyCode::Delete => screen.input.delete(),
        KeyCode::Left => screen.input.left(),
        KeyCode::Right => screen.input.right(),
        KeyCode::Up => screen.input.up(),
        KeyCode::Down => screen.input.down(),
        KeyCode::Home => screen.input.home(),
        KeyCode::End => screen.input.end(),
        _ => return false,
    }
    true
}
