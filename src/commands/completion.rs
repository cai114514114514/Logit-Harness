use super::{Service, Timing};

/// A displayable command/skill entry. `insert` is the exact safe completion;
/// duplicate skill names use their source ID instead of picking precedence.
#[derive(Clone, Debug)]
pub struct Completion {
    pub name: String,
    pub description: String,
    pub source: String,
    pub timing: Timing,
    pub insert: String,
}

impl Service<'_> {
    /// Discover Skills only when the client explicitly opens a slash menu. Merely
    /// constructing a service or rendering welcome/help does not scan the project.
    pub fn complete(&self, draft: &str) -> anyhow::Result<Vec<Completion>> {
        if !draft.starts_with('/') {
            return Ok(vec![]);
        }
        let mut entries = super::matches(draft)
            .into_iter()
            .map(|entry| Completion {
                name: entry.name.into(),
                description: entry.description.into(),
                source: entry.source.into(),
                timing: entry.timing,
                insert: format!("{}{}", entry.name, if entry.arguments { " " } else { "" }),
            })
            .collect::<Vec<_>>();
        let skills = self.skills()?;
        let explicit = draft.strip_prefix("/skills run ");
        let prefix = draft.split_whitespace().next().unwrap_or(draft);
        for entry in &skills.entries {
            let alias = format!("/{}", entry.name);
            let conflict = super::DIRECTORY.iter().any(|core| core.name == alias);
            let selected = if let Some(selector) = explicit {
                entry.id.starts_with(selector) || entry.name.starts_with(selector)
            } else {
                !conflict && alias.starts_with(prefix)
            };
            if !selected {
                continue;
            }
            let qualified = explicit.is_some() || skills.candidates(&entry.name).len() > 1;
            entries.push(Completion {
                name: alias,
                description: entry.description.clone(),
                source: format!("{} · {}", entry.source, entry.path.display()),
                timing: Timing::Runtime,
                insert: if qualified {
                    format!("/skills run {} ", entry.id)
                } else {
                    format!("/{} ", entry.name)
                },
            });
        }
        Ok(entries)
    }
}
