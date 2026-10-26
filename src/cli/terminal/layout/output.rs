/// Append styled text directly to the normal buffer. Converting a Line to a
/// String loses the blue brand and all transcript hierarchy in native history.
pub(super) fn line(
    output: &mut impl std::io::Write,
    line: &ratatui::text::Line<'_>,
) -> std::io::Result<()> {
    use crossterm::style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor};
    for span in &line.spans {
        let style = line.style.patch(span.style);
        crossterm::queue!(output, SetAttribute(Attribute::Reset), ResetColor)?;
        if let Some(color) = style.fg.filter(|_| !super::theme::plain()) {
            crossterm::queue!(output, SetForegroundColor(terminal_color(color)))?;
        }
        if style.add_modifier.contains(ratatui::style::Modifier::BOLD) {
            crossterm::queue!(output, SetAttribute(Attribute::Bold))?;
        }
        crossterm::queue!(output, Print(super::state::clean(&span.content)))?;
    }
    crossterm::queue!(output, SetAttribute(Attribute::Reset), ResetColor)?;
    output.flush()
}

fn terminal_color(color: ratatui::style::Color) -> crossterm::style::Color {
    use crossterm::style::Color as C;
    use ratatui::style::Color as R;
    match color {
        R::Reset => C::Reset,
        R::Black => C::Black,
        R::Red => C::DarkRed,
        R::Green => C::DarkGreen,
        R::Yellow => C::DarkYellow,
        R::Blue => C::DarkBlue,
        R::Magenta => C::DarkMagenta,
        R::Cyan => C::DarkCyan,
        R::Gray => C::Grey,
        R::DarkGray => C::DarkGrey,
        R::LightRed => C::Red,
        R::LightGreen => C::Green,
        R::LightYellow => C::Yellow,
        R::LightBlue => C::Blue,
        R::LightMagenta => C::Magenta,
        R::LightCyan => C::Cyan,
        R::White => C::White,
        R::Rgb(r, g, b) => C::Rgb { r, g, b },
        R::Indexed(value) => C::AnsiValue(value),
    }
}
