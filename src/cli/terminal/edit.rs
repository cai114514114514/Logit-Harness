use super::Screen;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Shared editor for the welcome screen and a running session. Returning false
/// leaves submission/lifecycle keys to the caller that owns the connection.
pub(super) fn edit(screen: &mut Screen, key: KeyEvent) -> bool {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let menu = screen.menu();
    let choices = if let Some(choice) = &screen.skill_choices {
        choice.entries.len()
    } else if screen.team_picker {
        screen.members.len() + 1
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
            screen.team_picker = false;
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
        KeyCode::Char('a') if control => screen.input.line_home(),
        KeyCode::Char('e') if control => screen.input.line_end(),
        KeyCode::Char('w') if control => screen.input.delete_word(),
        KeyCode::Char('k') if control => screen.input.kill_line(),
        KeyCode::Char('b') if alt => screen.input.word_left(),
        KeyCode::Char('f') if alt => screen.input.word_right(),
        KeyCode::Left if alt || control => screen.input.word_left(),
        KeyCode::Right if alt || control => screen.input.word_right(),
        KeyCode::Backspace if alt => screen.input.delete_word(),
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
        KeyCode::Up => screen.input.visual_up(editor_width()),
        KeyCode::Down => screen.input.visual_down(editor_width()),
        KeyCode::Home => screen.input.line_home(),
        KeyCode::End => screen.input.line_end(),
        _ => return false,
    }
    true
}

fn editor_width() -> usize {
    // Match the two outer margins, prompt, and composer padding in render.rs.
    crossterm::terminal::size()
        .map_or(80, |(width, _)| width)
        .saturating_sub(6) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_and_line_shortcuts_preserve_the_rest_of_the_draft() {
        let mut screen = Screen::default();
        screen.input.insert("保留第一行\nalpha beta");
        edit(
            &mut screen,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert_eq!(screen.input.text(), "保留第一行\nalpha ");
        edit(
            &mut screen,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        screen.input.insert("X");
        assert_eq!(screen.input.text(), "保留第一行\nXalpha ");
        edit(
            &mut screen,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
        );
        assert_eq!(screen.input.text(), "保留第一行\nX");
        screen.input.replace("alpha beta 👩‍💻");
        edit(
            &mut screen,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
        );
        screen.input.insert("X");
        assert_eq!(screen.input.text(), "alpha beta X👩‍💻");
    }
}
