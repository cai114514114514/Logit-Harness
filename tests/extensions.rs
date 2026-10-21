use loh::extensions::{instructions::Instructions, skills::Skills};

fn skill(root: &std::path::Path, folder: &str, name: &str, body: &str) {
    let directory = root.join(folder);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("SKILL.md"),format!("---\nname: {name}\ndescription: >\n  Useful project guidance\n  for real tasks.\n---\n{body}")).unwrap();
}

#[test]
fn skills_discover_metadata_but_load_only_the_selected_body_and_resources() {
    let project = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    skill(
        project.path(),
        ".agents/skills/review",
        "review",
        "Project instructions",
    );
    skill(user.path(), "review", "review", "User instructions");
    skill(
        project.path(),
        "node_modules/ignored",
        "ignored",
        "Should not discover",
    );
    let catalog = Skills::discover(project.path(), &[user.path().to_owned()]).unwrap();
    assert_eq!(catalog.entries.len(), 2);
    let metadata = serde_json::to_string(&catalog.entries).unwrap();
    assert!(!metadata.contains("Project instructions"));
    let project_skill = catalog
        .entries
        .iter()
        .find(|s| s.source == "project")
        .unwrap();
    assert!(project_skill.description.contains("for real tasks."));
    let loaded = catalog.load(&project_skill.id, None).unwrap();
    assert!(loaded.content.contains("Project instructions"));
    let reference = project.path().join(".agents/skills/review/references");
    std::fs::create_dir(&reference).unwrap();
    std::fs::write(reference.join("check.md"), "Referenced check").unwrap();
    assert_eq!(
        catalog
            .load(&project_skill.id, Some("references/check.md"))
            .unwrap()
            .content,
        "Referenced check"
    );
    assert!(
        catalog
            .load(&project_skill.id, Some("../../outside.md"))
            .is_err()
    );
    assert!(
        catalog
            .load(&project_skill.id, Some("/etc/passwd"))
            .is_err()
    );
}

#[test]
fn changed_or_symlinked_skills_are_not_silently_loaded_under_old_metadata() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    skill(root.path(), "skills/review", "review", "Initial");
    let catalog = Skills::discover(root.path(), &[]).unwrap();
    let entry = &catalog.entries[0];
    std::fs::write(outside.path().join("secret"), "not a skill resource").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret"),
        root.path().join("skills/review/link"),
    )
    .unwrap();
    assert!(catalog.load(&entry.id, Some("link")).is_err());
    skill(
        root.path(),
        "skills/review",
        "review",
        "Changed instructions",
    );
    assert!(
        catalog
            .load(&entry.id, None)
            .unwrap_err()
            .to_string()
            .contains("changed")
    );
}

#[test]
fn directory_instructions_apply_only_to_their_subtree_with_provenance() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("frontend")).unwrap();
    std::fs::create_dir(root.path().join("backend")).unwrap();
    std::fs::write(root.path().join("AGENTS.md"), "Use readable modules.").unwrap();
    std::fs::write(
        root.path().join("frontend/AGENTS.md"),
        "Use accessible controls.",
    )
    .unwrap();
    std::fs::write(
        root.path().join("backend/AGENTS.md"),
        "Keep SQL transactions short.",
    )
    .unwrap();
    let instructions = Instructions::new(root.path().to_owned());
    let applicable = instructions.for_paths(&["frontend/new.ts".into()]).unwrap();
    assert_eq!(applicable.len(), 2);
    assert_eq!(applicable[0].path, "AGENTS.md");
    assert_eq!(applicable[1].path, "frontend/AGENTS.md");
    assert!(applicable[1].content.contains("accessible"));
    assert!(!applicable.iter().any(|entry| entry.content.contains("SQL")));
    assert!(instructions.for_paths(&["../outside.rs".into()]).is_err());
}

#[tokio::test]
async fn patches_require_current_directory_rules_even_for_a_new_file() {
    use loh::{
        domain::Goal,
        extensions::knowledge::Knowledge,
        model::ToolCall,
        tools::{Scope, ToolHost, files::Workspace},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("frontend")).unwrap();
    std::fs::write(root.path().join("AGENTS.md"), "Use readable modules.").unwrap();
    std::fs::write(
        root.path().join("frontend/AGENTS.md"),
        "Use accessible controls.",
    )
    .unwrap();
    let knowledge = Arc::new(Knowledge::new(root.path().to_owned(), vec![]).unwrap());
    let goal = Goal::new("make a page");
    let mut host = ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(goal.clone())),
        logs.path().to_owned(),
    );
    host.knowledge = Some(knowledge.clone());
    for tool in loh::extensions::skills::tools(knowledge.clone()) {
        host.register(tool).unwrap();
    }
    let mut scope = Scope {
        requirements: goal.active_versions(),
        ..Default::default()
    };
    let prompt = knowledge.prompt(&mut scope).unwrap();
    assert!(prompt.contains("Use readable modules."));
    let call = ToolCall {
        id: "patch".into(),
        name: "apply_patch".into(),
        arguments: json!({"edits":[{"path":"frontend/page.ts","expected":"missing","operation":"create","content":"export const title = 'page';"}]}),
    };
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        host.execute(
            call.clone(),
            scope.clone(),
            CancellationToken::new(),
            events.clone()
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("read project_instructions")
    );
    let loaded = host
        .execute(
            ToolCall {
                id: "rules".into(),
                name: "project_instructions".into(),
                arguments: json!({"paths":["frontend/page.ts"]}),
            },
            scope.clone(),
            CancellationToken::new(),
            events.clone(),
        )
        .await
        .unwrap();
    scope.observed.extend(loaded.files);
    assert!(
        host.execute(call, scope, CancellationToken::new(), events)
            .await
            .unwrap()
            .success
    );
}
