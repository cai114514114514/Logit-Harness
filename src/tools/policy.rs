#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask(String),
    Deny(String),
}

/// An application policy, not a proof about arbitrary programs' side effects.
/// Explicit external mutations are surfaced to the user rather than hidden in shell text.
///
/// This is a heuristic over whitespace-split words and is known to be bypassable:
/// `git -c x=y push` shifts the verb out of the checked word pair, `curl -XPOST`
/// hides the method inside one token, `env sudo …` wraps a privileged command.
/// Defense in depth comes from the sandbox profile, not from this check.
/// The `curl -x` case is a deliberate false positive: after lowercasing, `-x`
/// (proxy) is indistinguishable from `-X` (method), and flagging a proxy use is
/// the safe direction.
pub fn shell(command: &str) -> Decision {
    let words: Vec<_> = command
        .split(|c: char| c.is_whitespace() || ";|&()".contains(c))
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_matches(['\'', '"']).to_lowercase())
        .collect();
    for pair in words.windows(2) {
        if matches!(
            (pair[0].as_str(), pair[1].as_str()),
            ("git", "push")
                | ("npm" | "pnpm" | "yarn" | "cargo", "publish")
                | ("gh", "release")
                | ("terraform", "apply" | "destroy")
                | ("kubectl", "apply" | "delete")
                | ("vercel", "deploy")
                | ("curl", "-x" | "--request")
        ) {
            return Decision::Ask("command includes an external mutation".into());
        }
    }
    if words
        .iter()
        .any(|s| matches!(s.as_str(), "sudo" | "shutdown" | "reboot"))
    {
        return Decision::Ask("command changes host privileges or machine state".into());
    }
    if words.iter().any(|s| s == "curl")
        && words.iter().any(|s| {
            s.starts_with("--data")
                || matches!(s.as_str(), "-d" | "-f" | "-t" | "--upload-file" | "--form")
        })
    {
        return Decision::Ask("HTTP upload requires explicit authorization".into());
    }
    Decision::Allow
}
