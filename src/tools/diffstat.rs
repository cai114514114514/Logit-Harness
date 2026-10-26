//! Exact added/removed line counts for a committed edit, or nothing.
//!
//! The terminal shows `+12 −3` beside a patch. A wrong count is worse than none,
//! so this computes a true shortest edit script (Myers) and gives up on inputs
//! whose difference is too large to measure cheaply.

/// Edit distance beyond which counting is abandoned. Ordinary patches differ by
/// a few hundred lines at most once the shared prefix and suffix are removed.
const MAX_DISTANCE: usize = 4096;

/// `(added, removed)` lines between two file contents; `None` for a missing side
/// is an empty file. Returns `None` when the difference is too large to count.
pub fn lines(before: Option<&[u8]>, after: Option<&[u8]>) -> Option<(u64, u64)> {
    let before = split(before.unwrap_or_default());
    let after = split(after.unwrap_or_default());
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    let (before, after) = (&before[prefix..], &after[prefix..]);
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (before, after) = (
        &before[..before.len() - suffix],
        &after[..after.len() - suffix],
    );
    let distance = if before.is_empty() || after.is_empty() {
        before.len() + after.len()
    } else {
        distance(before, after)?
    };
    // Every edit is one insertion or one deletion; the common part is the rest.
    let common = (before.len() + after.len() - distance) / 2;
    Some((
        (after.len() - common) as u64,
        (before.len() - common) as u64,
    ))
}

fn split(content: &[u8]) -> Vec<&[u8]> {
    if content.is_empty() {
        return vec![];
    }
    content
        .strip_suffix(b"\n")
        .unwrap_or(content)
        .split(|b| *b == b'\n')
        .collect()
}

/// Myers' greedy forward search for the length of the shortest edit script.
fn distance(a: &[&[u8]], b: &[&[u8]]) -> Option<usize> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let limit = (a.len() + b.len()).min(MAX_DISTANCE);
    let offset = limit as isize + 1;
    let mut furthest = vec![0isize; 2 * limit + 3];
    for d in 0..=limit as isize {
        for k in (-d..=d).step_by(2) {
            let index = (k + offset) as usize;
            let mut x = if k == -d || (k != d && furthest[index - 1] < furthest[index + 1]) {
                furthest[index + 1]
            } else {
                furthest[index - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            furthest[index] = x;
            if x >= n && y >= m {
                return Some(d as usize);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::lines;

    #[test]
    fn counts_match_a_minimal_line_diff() {
        assert_eq!(lines(None, Some(b"a\nb\nc\n")), Some((3, 0)));
        assert_eq!(lines(Some(b"a\nb\n"), None), Some((0, 2)));
        assert_eq!(lines(Some(b"a\nb\nc\n"), Some(b"a\nB\nc\n")), Some((1, 1)));
        assert_eq!(lines(Some(b"a\nc\n"), Some(b"a\nb\nc\nd\n")), Some((2, 0)));
        assert_eq!(lines(Some(b"x\na\nb\n"), Some(b"a\nb\nx\n")), Some((1, 1)));
        assert_eq!(lines(Some(b"same\n"), Some(b"same\n")), Some((0, 0)));
        // A missing final newline does not count as a changed line.
        assert_eq!(lines(Some(b"a\nb"), Some(b"a\nb\n")), Some((0, 0)));
    }

    #[test]
    fn an_unmeasurably_large_difference_has_no_count() {
        let before = (0..5000).map(|i| format!("a{i}\n")).collect::<String>();
        let after = (0..5000).map(|i| format!("b{i}\n")).collect::<String>();
        assert_eq!(lines(Some(before.as_bytes()), Some(after.as_bytes())), None);
    }
}
