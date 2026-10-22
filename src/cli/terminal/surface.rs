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
    modal: bool,
}

impl Surface {
    pub fn width(&self) -> u16 {
        crossterm::terminal::size().map_or(self.size.0, |s| s.0)
    }
    pub fn layout(&mut self, height: u16) -> std::io::Result<()> {
        if self.modal {
            return Ok(());
        }
        let size = crossterm::terminal::size()?;
        let height = height.max(1).min(size.1);
        if height == self.area.height && size == self.size {
            return Ok(());
        }
        let row = self.area.y.min(size.1.saturating_sub(1));
        crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::MoveTo(0, row),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown),
            crossterm::style::Print("\r\n".repeat(height.saturating_sub(1) as usize))
        )?;
        let overflow = row.saturating_add(height).saturating_sub(size.1);
        self.area = Rect::new(0, row.saturating_sub(overflow), size.0, height);
        self.size = size;
        self.reset(self.area)
    }
    pub fn modal(&mut self, modal: bool) -> std::io::Result<()> {
        if self.modal == modal {
            return Ok(());
        }
        if modal {
            crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
        } else {
            crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
        }
        self.modal = modal;
        let area = if modal {
            Rect::new(0, 0, self.size.0, self.size.1)
        } else {
            self.area
        };
        self.reset(area)?;
        Ok(())
    }
    /// Remove only the active region, append completed lines using normal LF,
    /// then reserve a fresh active region. No cursor query and no scroll-region CSI.
    pub fn append(&mut self, lines: &[ratatui::text::Line<'_>]) -> std::io::Result<()> {
        use crossterm::{
            cursor::MoveTo,
            style::Print,
            terminal::{Clear, ClearType},
        };
        if lines.is_empty() || self.modal {
            return Ok(());
        }
        let mut output = std::io::stdout();
        crossterm::execute!(
            output,
            MoveTo(0, self.area.y),
            Clear(ClearType::FromCursorDown)
        )?;
        for line in lines {
            crossterm::execute!(
                output,
                Print(super::state::clean(&line.to_string())),
                Print("\r\n")
            )?;
        }
        let size = crossterm::terminal::size()?;
        let row =
            (self.area.y as usize + lines.len()).min(size.1.saturating_sub(1) as usize) as u16;
        let height = self.area.height.min(size.1).max(1);
        crossterm::execute!(
            output,
            Print("\r\n".repeat(height.saturating_sub(1) as usize))
        )?;
        let overflow = row.saturating_add(height).saturating_sub(size.1);
        self.area = Rect::new(0, row.saturating_sub(overflow), size.0, height);
        self.size = size;
        self.reset(self.area)?;
        Ok(())
    }

    fn reset(&mut self, area: Rect) -> std::io::Result<()> {
        // Terminal::clear queries the cursor; Terminal::resize clears the entire
        // display on width shrink. Recreate only the fixed diff buffers instead.
        crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::MoveTo(area.x, area.y),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown)
        )?;
        self.terminal = ratatui::Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(std::io::stdout()),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )?;
        Ok(())
    }
    pub fn draw(&mut self, draw: impl FnOnce(&mut Frame)) -> std::io::Result<()> {
        let size = crossterm::terminal::size()?;
        if size != self.size {
            let height = self.area.height.min(size.1).max(1);
            self.area = Rect::new(
                0,
                self.area.y.min(size.1.saturating_sub(height)),
                size.0,
                height,
            );
            let area = if self.modal {
                Rect::new(0, 0, size.0, size.1)
            } else {
                self.area
            };
            self.reset(area)?;
            self.size = size;
        }
        self.terminal.draw(draw)?;
        Ok(())
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        if self.modal {
            let _ =
                crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        }
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
        modal: false,
    })
}

fn height(rows: u16, welcome: bool) -> u16 {
    let height = rows.saturating_sub(1).max(1);
    if welcome { height.min(19) } else { height }
}
