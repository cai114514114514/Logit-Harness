use loh::eval::manifest::{Frozen, Manifest};
use std::path::Path;

#[test]
fn authored_corpus_has_frozen_split_language_and_project_balance() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("eval/corpus");
    let manifest = Manifest::load(&corpus).unwrap();
    assert_eq!(manifest.tasks.len(), 36);
    for (split, count) in [("development", 12), ("holdout", 24)] {
        let tasks: Vec<_> = manifest
            .tasks
            .iter()
            .filter(|task| task.split == split)
            .collect();
        assert_eq!(tasks.len(), count);
        assert_eq!(
            tasks.iter().filter(|task| task.kind == "new").count(),
            count / 2
        );
        for language in ["python", "rust", "typescript"] {
            assert_eq!(
                tasks
                    .iter()
                    .filter(|task| task.language == language)
                    .count(),
                count / 3
            );
        }
    }
}

#[test]
fn freeze_is_immutable_and_rejects_modified_acceptance_code() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("eval/corpus");
    let root = tempfile::tempdir().unwrap();
    let corpus = root.path().join("corpus");
    for entry in walkdir::WalkDir::new(&source)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file() && !entry.path().to_string_lossy().contains("__pycache__")
        })
    {
        let path = corpus.join(entry.path().strip_prefix(&source).unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(entry.path(), path).unwrap();
    }
    let manifest = Manifest::load(&corpus).unwrap();
    let path = root.path().join("frozen.json");
    let frozen = manifest.freeze(&path).unwrap();
    assert_eq!(manifest.freeze(&path).unwrap().digest, frozen.digest);
    Frozen::load(&path).unwrap().verify(&corpus).unwrap();
    std::fs::write(corpus.join("accept.py"), "print('always pass')\n").unwrap();
    assert!(frozen.verify(&corpus).is_err());
    assert!(Manifest::load(&corpus).unwrap().freeze(&path).is_err());
}
