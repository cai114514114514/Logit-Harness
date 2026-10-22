use super::{Entry, Loaded, Skills};

const WORKFLOWS: &[(&str, &str, &str)] = &[
    (
        "init",
        "Inspect and initialize project guidance without overwriting existing AGENTS.md",
        include_str!("../../../skills/init/SKILL.md"),
    ),
    (
        "review",
        "Read-only review with concrete source evidence",
        include_str!("../../../skills/review/SKILL.md"),
    ),
];

impl Skills {
    pub fn with_bundled(mut self) -> Self {
        for (name, description, content) in WORKFLOWS {
            let id = format!("bundled:{name}");
            if self.entries.iter().any(|entry| entry.id == id) {
                continue;
            }
            self.entries.push(Entry {
                id,
                name: (*name).into(),
                description: (*description).into(),
                source: "bundled".into(),
                path: format!("bundled/{name}/SKILL.md").into(),
                hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
                stamp: String::new(),
            });
        }
        self.entries
            .sort_by(|a, b| (&a.source, &a.name, &a.path).cmp(&(&b.source, &b.name, &b.path)));
        self
    }
}

pub(super) fn load(id: &str, resource: Option<&str>) -> anyhow::Result<Option<Loaded>> {
    let Some((name, _, content)) = WORKFLOWS
        .iter()
        .find(|(name, _, _)| id == format!("bundled:{name}"))
    else {
        return Ok(None);
    };
    anyhow::ensure!(
        resource.is_none_or(|resource| resource == "SKILL.md"),
        "bundled skill has no such resource"
    );
    Ok(Some(Loaded {
        path: format!("bundled/{name}/SKILL.md").into(),
        hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
        content: (*content).into(),
    }))
}
