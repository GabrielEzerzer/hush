//! The directory rule: every tracked file sits under a known top-level folder or is a known top-level file.

use std::process::Command;

const TOP_LEVEL_FOLDERS: &[&str] = &[".github", "docs-assets", "harness", "setup", "src", "tests"];
const TOP_LEVEL_FILES: &[&str] = &[
    ".gitattributes",
    ".gitignore",
    ".lint-baseline.json",
    "Cargo.lock",
    "Cargo.toml",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "README.md",
    "SECURITY.md",
    "features.json",
    "rustfmt.toml",
];

fn misplaced(paths: &[&str]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| match path.split_once('/') {
            Some((folder, _)) => !TOP_LEVEL_FOLDERS.contains(&folder),
            None => !TOP_LEVEL_FILES.contains(path),
        })
        .map(|path| path.to_string())
        .collect()
}

#[test]
fn every_tracked_file_sits_in_a_known_place() {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git must be on PATH to list the tracked files");
    assert!(
        out.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let listed = String::from_utf8(out.stdout).expect("tracked paths are UTF-8");
    let paths: Vec<&str> = listed.split('\0').filter(|p| !p.is_empty()).collect();
    assert!(paths.contains(&"Cargo.toml"), "git ls-files did not list the repo root");
    let bad = misplaced(&paths);
    assert!(
        bad.is_empty(),
        "these tracked files are outside the known layout: {bad:?}\n\
         Move each one under a top-level folder {TOP_LEVEL_FOLDERS:?}, or into the top-level files {TOP_LEVEL_FILES:?}.\n\
         A new top-level folder or file is a layout change: add it to the list in tests/layout.rs in the same PR."
    );
}

#[test]
fn the_rule_flags_unknown_top_level_files_and_folders() {
    assert_eq!(
        misplaced(&[
            "src/main.rs",
            "README.md",
            "notes.md",
            "scripts/x.sh",
            "harness/evals/dev/behaviors.py"
        ]),
        ["notes.md", "scripts/x.sh"]
    );
}
