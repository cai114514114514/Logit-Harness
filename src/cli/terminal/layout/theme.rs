//! Colour tokens, chosen once per process.
//!
//! A light background gets a darker accent, because the brand blue is about
//! 2.8:1 on white. `NO_COLOR` removes colour at the two places output leaves the
//! program (the active frame and committed lines), so a new style cannot leak
//! past it; bold and underline remain, and bar weight still tells speakers apart.
use ratatui::{buffer::Buffer, style::Color};
use std::{
    io::{IsTerminal, Write},
    sync::{
        Mutex, Once, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const ACCENT_DARK: Color = Color::Rgb(91, 156, 255);
/// About 6.3:1 on white.
const ACCENT_LIGHT: Color = Color::Rgb(0, 92, 197);
const QUERY_TIMEOUT: Duration = Duration::from_millis(150);

static LIGHT: AtomicBool = AtomicBool::new(false);
static TYPED: Mutex<String> = Mutex::new(String::new());

/// Brand mark, your bar, selection, work in progress.
pub fn accent() -> Color {
    if LIGHT.load(Ordering::Relaxed) {
        ACCENT_LIGHT
    } else {
        ACCENT_DARK
    }
}
/// Secondary text, the main agent's bar, the footer.
pub fn muted() -> Color {
    Color::DarkGray
}
/// Current evidence, accepted clauses, successful steps.
pub fn fresh() -> Color {
    Color::Green
}
/// Evidence that is no longer current; revision cards.
pub fn stale() -> Color {
    Color::Yellow
}
/// Failed checks and steps, rejected receipts.
pub fn fail() -> Color {
    Color::Red
}
/// Members in stable roster order. Identity is always carried by the name too.
pub fn member(index: usize) -> Color {
    [Color::Cyan, Color::Magenta][index % 2]
}

/// <https://no-color.org>: present and not empty.
pub fn plain() -> bool {
    static PLAIN: OnceLock<bool> = OnceLock::new();
    *PLAIN.get_or_init(|| std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()))
}

/// Remove colour from a rendered frame under `NO_COLOR`.
pub(super) fn strip(buffer: &mut Buffer) {
    if !plain() {
        return;
    }
    for cell in &mut buffer.content {
        cell.set_fg(Color::Reset);
        cell.set_bg(Color::Reset);
    }
}

/// Text typed while the background query was reading stdin, for the editor.
pub(super) fn take_typed() -> String {
    std::mem::take(&mut TYPED.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Ask the terminal for its background once. Must run in raw mode and before
/// the async event reader exists: the two cannot share stdin (see surface.rs).
pub(super) fn detect() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if plain() || !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return;
        }
        let reply = query();
        let parsed = parse(&reply);
        if parsed.background.is_some_and(|rgb| luminance(rgb) > 0.5) {
            LIGHT.store(true, Ordering::Relaxed);
        }
        *TYPED.lock().unwrap_or_else(|e| e.into_inner()) = parsed.typed;
    });
}

/// OSC 11 asks for the background; DA1 follows because every terminal answers
/// it, so a terminal without OSC 11 support ends the wait immediately instead of
/// costing the whole timeout.
fn query() -> Vec<u8> {
    let mut stdout = std::io::stdout();
    if stdout
        .write_all(b"\x1b]11;?\x07\x1b[c")
        .and_then(|_| stdout.flush())
        .is_err()
    {
        return vec![];
    }
    let deadline = Instant::now() + QUERY_TIMEOUT;
    let mut reply = vec![];
    let mut buffer = [0u8; 256];
    while !parse(&reply).complete {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let mut fd = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&mut fd, 1, left.as_millis().max(1) as libc::c_int) };
        if ready <= 0 {
            break;
        }
        // SAFETY: the buffer is valid for `buffer.len()` bytes.
        let read =
            unsafe { libc::read(libc::STDIN_FILENO, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read <= 0 {
            break;
        }
        reply.extend_from_slice(&buffer[..read as usize]);
    }
    reply
}

#[derive(Debug, Default, PartialEq)]
struct Reply {
    background: Option<[f64; 3]>,
    /// The DA1 answer arrived; nothing more is coming.
    complete: bool,
    /// Printable input that was not part of either answer.
    typed: String,
}

fn parse(bytes: &[u8]) -> Reply {
    let mut reply = Reply::default();
    let mut other = vec![];
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        if let Some(body) = rest.strip_prefix(b"\x1b]11;") {
            let end = body
                .iter()
                .position(|b| *b == 0x07)
                .map(|p| (p, 1))
                .or_else(|| body.windows(2).position(|w| w == b"\x1b\\").map(|p| (p, 2)));
            let Some((end, terminator)) = end else {
                break;
            };
            reply.background = rgb(&body[..end]);
            i += 5 + end + terminator;
        } else if let Some(body) = rest.strip_prefix(b"\x1b[?") {
            let Some(end) = body.iter().position(|b| *b == b'c') else {
                break;
            };
            reply.complete = true;
            i += 3 + end + 1;
        } else if bytes[i] == 0x1b {
            // Any other escape sequence is a key such as an arrow or an unrelated
            // report, never text for the editor: skip it whole.
            i += escape_length(rest);
        } else {
            other.push(bytes[i]);
            i += 1;
        }
    }
    reply.typed = String::from_utf8_lossy(&other)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    reply
}

/// Length of the escape sequence at the start of `bytes` (which begins with ESC):
/// CSI up to its final byte, SS3 plus one byte, string sequences up to BEL or ST.
fn escape_length(bytes: &[u8]) -> usize {
    let end = |found: Option<usize>| found.map_or(bytes.len(), |n| n + 1);
    match bytes.get(1) {
        None => 1,
        Some(b'[') => end(bytes
            .iter()
            .skip(2)
            .position(|b| (0x40..=0x7e).contains(b))
            .map(|n| n + 2)),
        Some(b'O') => bytes.len().min(3),
        Some(b']' | b'P' | b'_' | b'^') => end(bytes
            .windows(2)
            .position(|w| w == b"\x1b\\")
            .map(|n| n + 1)
            .or_else(|| bytes.iter().position(|b| *b == 0x07))),
        Some(_) => 2,
    }
}

/// `rgb:R/G/B` with 1–4 hex digits per channel, scaled to 0..=1.
fn rgb(body: &[u8]) -> Option<[f64; 3]> {
    let text = std::str::from_utf8(body).ok()?.strip_prefix("rgb:")?;
    let channels = text
        .split('/')
        .map(|part| {
            let digits = part.len() as u32;
            if !(1..=4).contains(&digits) {
                return None;
            }
            let value = u32::from_str_radix(part, 16).ok()?;
            Some(f64::from(value) / f64::from(16u32.pow(digits) - 1))
        })
        .collect::<Option<Vec<_>>>()?;
    <[f64; 3]>::try_from(channels).ok()
}

/// WCAG relative luminance.
fn luminance([r, g, b]: [f64; 3]) -> f64 {
    let linear = |c: f64| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

#[cfg(test)]
mod tests {
    use super::{luminance, parse, rgb};

    #[test]
    fn background_replies_parse_with_either_terminator_and_any_width() {
        let white = parse(b"\x1b]11;rgb:ffff/ffff/ffff\x07\x1b[?62;22c");
        assert_eq!(white.background, Some([1.0, 1.0, 1.0]));
        assert!(white.complete && white.typed.is_empty());
        let dark = parse(b"\x1b]11;rgb:1e/1e/1e\x1b\\\x1b[?1;2c");
        assert!(luminance(dark.background.unwrap()) < 0.5);
        assert_eq!(rgb(b"rgb:f/0/0"), Some([1.0, 0.0, 0.0]));
        assert_eq!(rgb(b"rgb:fffff/0/0"), None);
        assert_eq!(rgb(b"rgba:ff/ff/ff/ff"), None);
    }

    #[test]
    fn a_terminal_without_osc_11_still_ends_the_wait() {
        let reply = parse(b"\x1b[?1;2c");
        assert!(reply.complete);
        assert_eq!(reply.background, None);
    }

    #[test]
    fn keys_typed_during_the_query_are_kept_for_the_editor() {
        let reply = parse("修\x1b]11;rgb:ffff/ffff/ffff\x07复\x1b[?62c".as_bytes());
        assert_eq!(reply.typed, "修复");
        let partial = parse(b"ab\x1b]11;rgb:ff");
        assert!(!partial.complete);
        // Arrow keys and other reports are not text.
        let keys = parse(b"a\x1b[A\x1bOBb\x1b[1;5C\x1b]11;rgb:0/0/0\x07c\x1b[?62c");
        assert_eq!(keys.typed, "abc");
    }

    #[test]
    fn light_and_dark_backgrounds_fall_on_either_side_of_the_threshold() {
        assert!(luminance([1.0, 1.0, 1.0]) > 0.5);
        assert!(luminance([0.98, 0.96, 0.9]) > 0.5);
        assert!(luminance([0.12, 0.12, 0.12]) < 0.5);
        assert!(luminance([0.0, 0.17, 0.21]) < 0.5);
    }
}
