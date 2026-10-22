//! Own a region of the normal terminal buffer, retaining shell scrollback.
use ratatui::{DefaultTerminal, Frame, TerminalOptions, Viewport, layout::Rect};
use std::sync::atomic::{AtomicU16, Ordering};

// Crossterm's async event reader and synchronous cursor queries cannot share
// stdin. Query once before EventStream exists, then retain the cursor position
// we set when switching from welcome to a conversation (or between sessions).
const UNKNOWN: u16 = u16::MAX;
static NEXT_ROW: AtomicU16 = AtomicU16::new(UNKNOWN);

pub(super) struct Surface {
    terminal: DefaultTerminal,
    area: Rect,
    size: (u16, u16),
    welcome: bool,
}

impl Surface {
    pub fn draw(&mut self, draw: impl FnOnce(&mut Frame)) -> std::io::Result<()> {
        let size = crossterm::terminal::size()?;
        if size != self.size {
            let height = height(size.1, self.welcome);
            self.area = Rect::new(
                0,
                self.area.y.min(size.1.saturating_sub(height)),
                size.0,
                height,
            );
            self.terminal.resize(self.area)?;
            self.size = size;
        }
        self.terminal.draw(draw)?;
        Ok(())
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        let row = self.area.bottom().saturating_sub(1);
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::MoveTo(0, row),
            crossterm::style::Print("\r\n")
        );
        NEXT_ROW.store(
            self.area.bottom().min(self.size.1.saturating_sub(1)),
            Ordering::SeqCst,
        );
    }
}

pub(super) fn open(welcome: bool) -> std::io::Result<Surface> {
    crossterm::terminal::enable_raw_mode()?;
    let size = crossterm::terminal::size()?;
    let saved = NEXT_ROW.load(Ordering::SeqCst);
    let row = if saved == UNKNOWN {
        crossterm::cursor::position()?.1
    } else {
        saved.min(size.1.saturating_sub(1))
    };
    let height = height(size.1, welcome);
    // Reserve rows using normal newlines; overflow moves earlier shell output
    // into native scrollback instead of clearing it or entering the alt screen.
    crossterm::execute!(
        std::io::stdout(),
        crossterm::cursor::MoveTo(0, row),
        crossterm::style::Print("\r\n".repeat(height.saturating_sub(1) as usize))
    )?;
    let overflow = row.saturating_add(height).saturating_sub(size.1);
    let area = Rect::new(0, row.saturating_sub(overflow), size.0, height);
    let terminal = ratatui::try_init_with_options(TerminalOptions {
        viewport: Viewport::Fixed(area),
    })?;
    Ok(Surface {
        terminal,
        area,
        size,
        welcome,
    })
}

fn height(rows: u16, welcome: bool) -> u16 {
    let height = rows.saturating_sub(1).max(1);
    if welcome { height.min(19) } else { height }
}
