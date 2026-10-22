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
    docked: bool,
    /// First reusable row after committed output. This can sit above the editor
    /// after a menu closes; that space belongs to the next append, not history.
    committed: u16,
}

impl Surface {
    pub fn present(
        &mut self,
        height: u16,
        conversation: bool,
        lines: &[ratatui::text::Line<'_>],
    ) -> std::io::Result<()> {
        if crossterm::terminal::size()? != self.size {
            self.layout(self.area.height, conversation)?;
        }
        // Settlement replaces its active preview. Append using the new height,
        // consuming any free rows left by a menu/editor that has since shrunk.
        if self.docked && !lines.is_empty() {
            self.append_height(lines, height)?;
        } else {
            self.layout(height, conversation)?;
            self.append(lines)?;
        }
        Ok(())
    }
    pub fn width(&self) -> u16 {
        crossterm::terminal::size().map_or(self.size.0, |s| s.0)
    }
    pub fn layout(&mut self, height: u16, conversation: bool) -> std::io::Result<()> {
        if self.modal {
            return Ok(());
        }
        let size = crossterm::terminal::size()?;
        let height = height.max(1).min(size.1);
        let entering = conversation && !self.docked;
        self.docked |= conversation;
        if height == self.area.height && size == self.size && !entering {
            return Ok(());
        }
        // The terminal clips/reflows its normal buffer on a height shrink.
        // Track that movement before reserving the new bottom region, otherwise
        // the old editor can be mistaken for settled output and left in history.
        let row = self
            .area
            .y
            .saturating_sub(self.size.1.saturating_sub(size.1))
            .min(size.1.saturating_sub(1));
        self.committed = self
            .committed
            .saturating_sub(self.size.1.saturating_sub(size.1))
            .min(size.1.saturating_sub(1));
        if self.docked {
            let top = size.1.saturating_sub(height);
            // Erase only the old active region. Shrink leaves reusable rows;
            // growing consumes them before it moves any settled output. Never
            // reverse-index history: RI would insert blanks at its boundary.
            crossterm::execute!(
                std::io::stdout(),
                crossterm::cursor::MoveTo(0, row),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown)
            )?;
            if top < self.committed {
                crossterm::execute!(
                    std::io::stdout(),
                    crossterm::cursor::MoveTo(0, size.1.saturating_sub(1)),
                    crossterm::style::Print("\r\n".repeat((self.committed - top) as usize))
                )?;
                self.committed = top;
            } else if entering {
                // Only the initial transition from welcome positions the first
                // conversation at the bottom. Later transitions retain output.
                self.committed = top;
            }
            self.area = Rect::new(0, top, size.0, height);
            self.size = size;
            return self.reset(self.area);
        }
        crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::MoveTo(0, row),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown),
            crossterm::style::Print("\r\n".repeat(height.saturating_sub(1) as usize))
        )?;
        let overflow = row.saturating_add(height).saturating_sub(size.1);
        self.area = Rect::new(0, row.saturating_sub(overflow), size.0, height);
        self.committed = self.committed.saturating_sub(overflow);
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
        self.append_height(lines, self.area.height)
    }

    fn append_height(
        &mut self,
        lines: &[ratatui::text::Line<'_>],
        height: u16,
    ) -> std::io::Result<()> {
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
            MoveTo(0, self.committed),
            Clear(ClearType::FromCursorDown)
        )?;
        for line in lines {
            super::output::line(&mut output, line)?;
            crossterm::execute!(output, Print("\r\n"))?;
        }
        let size = crossterm::terminal::size()?;
        self.committed =
            (self.committed as usize + lines.len()).min(size.1.saturating_sub(1) as usize) as u16;
        let height = height.min(size.1).max(1);
        let top = if self.docked {
            let top = size.1.saturating_sub(height);
            if self.committed > top {
                crossterm::execute!(
                    output,
                    MoveTo(0, size.1.saturating_sub(1)),
                    Print("\r\n".repeat((self.committed - top) as usize))
                )?;
                self.committed = top;
            }
            top
        } else {
            crossterm::execute!(
                output,
                Print("\r\n".repeat(height.saturating_sub(1) as usize))
            )?;
            let overflow = self.committed.saturating_add(height).saturating_sub(size.1);
            self.committed = self.committed.saturating_sub(overflow);
            self.committed
        };
        self.area = Rect::new(0, top, size.0, height);
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
        if self.modal {
            // A history-browser resize must not change our saved normal-buffer
            // geometry. On return, layout accounts for that resize exactly once.
            let area = Rect::new(0, 0, size.0, size.1);
            if self.terminal.get_frame().area() != area {
                self.reset(area)?;
            }
            self.terminal.draw(draw)?;
            return Ok(());
        }
        if size != self.size {
            self.layout(self.area.height, self.docked)?;
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
        docked: !welcome,
        committed: area.y,
    })
}

fn height(rows: u16, welcome: bool) -> u16 {
    if welcome { 1 } else { rows.max(1) }
}
