//! `hush.toml`: the committed file that names a repo's vault project. It holds no secrets.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const FILE: &str = "hush.toml";

#[derive(Serialize, Deserialize)]
pub struct ProjectFile {
    pub project: String,
}

pub fn read(dir: &Path) -> Result<Option<ProjectFile>> {
    let path = dir.join(FILE);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(
            toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))?,
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// The nearest hush.toml in `start` or any folder above it, up to the repo root: the first folder
/// holding `.git` (a folder, or a file in a worktree). A stray hush.toml above the repo is ignored.
pub fn find(start: &Path) -> Result<Option<ProjectFile>> {
    for dir in start.ancestors() {
        if let Some(f) = read(dir)? {
            return Ok(Some(f));
        }
        if dir.join(".git").exists() {
            break;
        }
    }
    Ok(None)
}

pub fn write(dir: &Path, project: &str) -> Result<()> {
    let body = format!(
        "# This repo's secrets live in the hush vault under this project name, not in the repo.\n{}",
        toml::to_string(&ProjectFile {
            project: project.to_owned()
        })?
    );
    fs::write(dir.join(FILE), body).context("writing hush.toml")
}

fn git(dir: &Path, args: &[&str]) -> Option<PathBuf> {
    let out = Command::new("git").args(args).current_dir(dir).output().ok()?;
    out.status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// The repo's top folder, or `cwd` outside a git repo.
pub fn repo_root(cwd: &Path) -> PathBuf {
    git(cwd, &["rev-parse", "--show-toplevel"]).unwrap_or_else(|| cwd.to_path_buf())
}

/// The main checkout's folder name, lowercased, so a worktree such as `web-app-fix-x` still
/// suggests `web-app`.
pub fn default_name(root: &Path) -> String {
    let main = git(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .and_then(|common| common.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| root.to_path_buf());
    main.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

const IGNORE_MARKER: &str = "# hush:";
const IGNORE_BLOCK: &str =
    "# hush: secrets live in the hush vault, never in files in the repo\n.env\n.env.*\n!.env.example\n";

/// Adds .env patterns to .gitignore once. Returns whether the file changed.
pub fn ignore_env_files(root: &Path) -> Result<bool> {
    let path = root.join(".gitignore");
    let current = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).context("reading .gitignore"),
    };
    if current.lines().any(|l| l.starts_with(IGNORE_MARKER)) {
        return Ok(false);
    }
    let sep = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let gap = if current.is_empty() { "" } else { "\n" };
    fs::write(&path, format!("{current}{sep}{gap}{IGNORE_BLOCK}")).context("writing .gitignore")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitignore_block_is_added_once() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target/").unwrap();
        assert!(ignore_env_files(dir.path()).unwrap());
        assert!(!ignore_env_files(dir.path()).unwrap());
        let text = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(text.starts_with("target/\n\n# hush:"));
        assert_eq!(text.matches(".env.*").count(), 1);
    }

    #[test]
    fn find_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "web-app").unwrap();
        let deep = dir.path().join("a").join("b");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(find(&deep).unwrap().unwrap().project, "web-app");
    }

    #[test]
    fn find_stops_at_the_repo_root() {
        for git_is_file in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            write(dir.path(), "stray").unwrap();
            let repo = dir.path().join("repo");
            let sub = repo.join("sub");
            fs::create_dir_all(&sub).unwrap();
            if git_is_file {
                fs::write(repo.join(".git"), "gitdir: elsewhere\n").unwrap();
            } else {
                fs::create_dir(repo.join(".git")).unwrap();
            }
            assert!(find(&sub).unwrap().is_none(), "found the hush.toml above the repo");
            write(&repo, "root").unwrap();
            assert_eq!(find(&sub).unwrap().unwrap().project, "root");
            write(&sub, "app").unwrap();
            assert_eq!(find(&sub).unwrap().unwrap().project, "app");
        }
    }
}
