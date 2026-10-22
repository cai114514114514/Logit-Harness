use std::{
    io::Read,
    path::{Path, PathBuf},
};

fn collect(root: &Path, paths: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, paths);
        } else {
            paths.push(path);
        }
    }
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut paths = vec![
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("build.rs"),
        root.join("skills/init/SKILL.md"),
        root.join("skills/review/SKILL.md"),
    ];
    collect(&root.join("src"), &mut paths);
    paths.sort();
    let mut digest = blake3::Hasher::new();
    let mut buffer = [0; 65536];
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        digest.update(
            path.strip_prefix(&root)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes(),
        );
        digest.update(&[0]);
        let mut file = std::fs::File::open(path).unwrap();
        loop {
            let count = file.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
    }
    println!("cargo:rerun-if-changed=src");
    println!(
        "cargo:rustc-env=LOH_SOURCE_HASH={}",
        digest.finalize().to_hex()
    );
}
