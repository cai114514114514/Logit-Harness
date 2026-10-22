//! Explicit clipboard adapter. Tests use a byte sink, never the user's clipboard.
use std::io::Write;
pub fn write(text: &str, sink: &mut impl Write) -> std::io::Result<()> {
    sink.write_all(text.as_bytes())
}
pub(super) fn copy(text: &str) -> anyhow::Result<()> {
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("clipboard stdin unavailable"))?;
    write(text, &mut input)?;
    drop(input);
    let result = child.wait_with_output()?;
    anyhow::ensure!(
        result.status.success(),
        "clipboard: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn sends_exact_bytes_without_touching_system_clipboard() {
        let text = "中文\n# **raw**\t\u{1b}[31m";
        let mut bytes = vec![];
        super::write(text, &mut bytes).unwrap();
        assert_eq!(bytes, text.as_bytes());
    }
}
