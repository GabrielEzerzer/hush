//! End-to-end tests against the built binary. Every test gets its own vault in a temp folder and
//! a fixed HUSH_KEY, so no test ever reads or writes the real OS keychain or the real vault.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use tempfile::TempDir;

const KEY_A: &str = "00112233-44556677-8899aabb-ccddeeff-00112233-44556677-8899aabb-ccddeeff";
const KEY_B: &str = "ffeeddcc-bbaa9988-77665544-33221100-ffeeddcc-bbaa9988-77665544-33221100";

struct Env {
    home: TempDir,
    repo: TempDir,
}

struct Run {
    out: Output,
}

impl Run {
    fn ok(self) -> Self {
        assert!(
            self.out.status.success(),
            "expected success, got {:?}\nstderr: {}",
            self.out.status,
            self.stderr()
        );
        self
    }
    fn fails(self) -> Self {
        assert!(
            !self.out.status.success(),
            "expected failure\nstdout: {}",
            self.stdout()
        );
        self
    }
    fn stdout(&self) -> String {
        String::from_utf8(self.out.stdout.clone()).unwrap()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.out.stderr).into_owned()
    }
    fn stderr_has(self, needle: &str) -> Self {
        assert!(
            self.stderr().contains(needle),
            "stderr lacks {needle:?}: {}",
            self.stderr()
        );
        self
    }
}

impl Env {
    fn new() -> Env {
        let env = Env {
            home: TempDir::new().unwrap(),
            repo: TempDir::new().unwrap(),
        };
        env.hush(&["setup"]).ok();
        env
    }

    fn with_project(name: &str) -> Env {
        let env = Env::new();
        env.hush(&["init", name]).ok();
        env
    }

    fn cmd(&self, key: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_hush"));
        c.current_dir(self.repo.path())
            .env("HUSH_HOME", self.home.path())
            .env("HUSH_KEY", key)
            .env_remove("HUSH_PROJECT")
            .env_remove("HUSH_ENV")
            .env_remove("CLAUDECODE");
        c
    }

    fn run_with(&self, mut c: Command, args: &[&str], stdin: Option<&[u8]>) -> Run {
        c.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped());
        let mut child = c.spawn().unwrap();
        {
            use std::io::Write;
            let mut pipe = child.stdin.take().unwrap();
            if let Some(input) = stdin {
                pipe.write_all(input).unwrap();
            }
        }
        Run {
            out: child.wait_with_output().unwrap(),
        }
    }

    fn hush(&self, args: &[&str]) -> Run {
        self.run_with(self.cmd(KEY_A), args, None)
    }

    fn hush_in(&self, args: &[&str], stdin: &str) -> Run {
        self.run_with(self.cmd(KEY_A), args, Some(stdin.as_bytes()))
    }

    fn set(&self, args: &[&str], value: &str) {
        let mut all = vec!["set"];
        all.extend_from_slice(args);
        self.hush_in(&all, &format!("{value}\n")).ok();
    }

    fn get(&self, args: &[&str]) -> String {
        let mut all = vec!["get"];
        all.extend_from_slice(args);
        self.hush(&all).ok().stdout()
    }

    fn file(&self, project: &str, env: &str) -> PathBuf {
        self.home
            .path()
            .join("projects")
            .join(project)
            .join(format!("{env}.hush"))
    }

    fn json(&self, env: &str) -> BTreeMap<String, String> {
        serde_json::from_str(&self.hush(&["export", "--format", "json", "-e", env]).ok().stdout()).unwrap()
    }
}

fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(all_files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn setup_is_idempotent_and_rejects_a_different_key() {
    let env = Env::new();
    env.hush(&["setup"]).ok().stderr_has("matches");
    env.run_with(env.cmd(KEY_B), &["setup"], None)
        .fails()
        .stderr_has("does not match");
    env.run_with(env.cmd("not-a-key"), &["setup"], None)
        .fails()
        .stderr_has("HUSH_KEY");
}

#[test]
fn commands_before_setup_point_to_setup() {
    let env = Env {
        home: TempDir::new().unwrap(),
        repo: TempDir::new().unwrap(),
    };
    env.hush(&["init", "demo"]).fails().stderr_has("hush setup");
    env.hush(&["-p", "demo", "list"]).fails().stderr_has("hush setup");
}

#[test]
fn init_links_the_repo_and_creates_the_project() {
    let env = Env::with_project("demo");
    let toml = fs::read_to_string(env.repo.path().join("hush.toml")).unwrap();
    assert!(toml.contains("project = \"demo\""), "{toml}");
    let ignore = fs::read_to_string(env.repo.path().join(".gitignore")).unwrap();
    assert!(
        ignore.contains(".env.*") && ignore.contains("!.env.example"),
        "{ignore}"
    );
    assert!(
        env.hush(&["project", "list"])
            .ok()
            .stdout()
            .contains("demo  (no secrets yet)")
    );

    env.hush(&["init"]).ok().stderr_has("Linked to existing project demo");
    env.hush(&["init", "other"]).fails().stderr_has("already links");
    assert_eq!(fs::read_to_string(env.repo.path().join(".gitignore")).unwrap(), ignore);

    // A subfolder of the repo finds hush.toml by walking up.
    let sub = env.repo.path().join("apps").join("web");
    fs::create_dir_all(&sub).unwrap();
    env.set(&["A"], "1");
    let mut c = env.cmd(KEY_A);
    c.current_dir(&sub);
    assert_eq!(env.run_with(c, &["list"], None).ok().stdout(), "A\n");
}

#[test]
fn init_in_a_subfolder_gives_that_app_its_own_project() {
    let env = Env::new();
    let repo = env.repo.path().join("myrepo");
    let app = repo.join("app");
    fs::create_dir_all(&app).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );

    let mut c = env.cmd(KEY_A);
    c.current_dir(&repo);
    env.run_with(c, &["init"], None).ok().stderr_has("project myrepo");
    let mut c = env.cmd(KEY_A);
    c.current_dir(&app);
    env.run_with(c, &["init"], None).ok().stderr_has("project myrepo-app");

    assert!(app.join("hush.toml").exists() && repo.join("hush.toml").exists());
    assert!(
        !app.join(".gitignore").exists(),
        ".env patterns belong in the repo root's .gitignore"
    );
    let mut c = env.cmd(KEY_A);
    c.current_dir(&app);
    env.run_with(
        c,
        &["set", "ONLY_APP"],
        Some(
            b"1
",
        ),
    )
    .ok()
    .stderr_has("myrepo-app/dev");
    let projects = env.hush(&["project", "list"]).ok().stdout();
    assert!(
        projects.contains("myrepo  (no secrets yet)") && projects.contains("myrepo-app  (dev)"),
        "{projects}"
    );
}

#[test]
fn set_get_list_rm() {
    let env = Env::with_project("demo");
    env.set(&["API_KEY"], "first");
    env.hush_in(&["set", "API_KEY"], "second\n")
        .ok()
        .stderr_has("Updated API_KEY");
    env.set(&["DB_URL"], "postgres://localhost/x");
    assert_eq!(env.get(&["API_KEY"]), "second");

    let listed = env.hush(&["list"]).ok().stdout();
    assert_eq!(listed, "API_KEY\nDB_URL\n");
    assert!(!listed.contains("second"));

    env.hush(&["rm", "API_KEY"]).ok();
    env.hush(&["get", "API_KEY"])
        .fails()
        .stderr_has("no secret API_KEY in demo/dev");
    env.hush(&["rm", "API_KEY"]).fails();
    assert_eq!(env.hush(&["ls"]).ok().stdout(), "DB_URL\n");
    assert!(env.hush(&["project", "list"]).ok().stdout().contains("demo  (dev)"));
}

#[test]
fn windows_line_endings_on_stdin_are_stripped_once() {
    let env = Env::with_project("demo");
    env.hush_in(&["set", "A"], "value\r\n").ok();
    assert_eq!(env.get(&["A"]), "value");
    env.hush_in(&["set", "B"], "line1\nline2\n").ok();
    assert_eq!(env.get(&["B"]), "line1\nline2");
}

#[test]
fn values_are_never_taken_from_arguments() {
    let env = Env::with_project("demo");
    env.hush(&["set", "API_KEY", "leaky-value"]).fails();
    env.hush(&["get", "API_KEY"]).fails();
}

#[test]
fn plaintext_never_reaches_the_disk() {
    let env = Env::with_project("demo");
    env.set(&["CANARY_NAME_ZQX"], "canary-value-8c1f2e");
    let dotenv = env.repo.path().join(".env.local");
    fs::write(&dotenv, "IMPORTED_NAME_ZQX=imported-value-77d1\n").unwrap();
    env.hush(&["import", dotenv.to_str().unwrap()]).ok();
    env.set(&["-e", "prod", "PROD_NAME_ZQX"], "prod-value-a91c");

    let files = all_files(env.home.path());
    assert!(files.len() >= 4, "{files:?}");
    for f in files {
        let bytes = fs::read(&f).unwrap();
        for needle in [
            "canary-value-8c1f2e",
            "imported-value-77d1",
            "prod-value-a91c",
            "CANARY_NAME_ZQX",
            "IMPORTED_NAME_ZQX",
            "PROD_NAME_ZQX",
        ] {
            assert!(
                !contains(&bytes, needle.as_bytes()),
                "{needle} found in plaintext in {}",
                f.display()
            );
        }
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            matches!(name.as_str(), "vault.toml" | ".lock" | "dev.hush" | "prod.hush"),
            "unexpected file {name}"
        );
    }
}

#[test]
fn a_wrong_key_fails_before_anything_is_written() {
    let env = Env::with_project("demo");
    env.set(&["A"], "1");
    let before = fs::read(env.file("demo", "dev")).unwrap();
    for args in [
        &["set", "B"][..],
        &["rm", "A"],
        &["get", "A"],
        &["list"],
        &["run", "--", "x"],
    ] {
        env.run_with(env.cmd(KEY_B), args, Some(b"2\n"))
            .fails()
            .stderr_has("does not match");
    }
    assert_eq!(fs::read(env.file("demo", "dev")).unwrap(), before);
}

#[test]
fn a_modified_file_is_rejected_and_not_overwritten() {
    let env = Env::with_project("demo");
    env.set(&["A"], "1");
    let path = env.file("demo", "dev");
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    fs::write(&path, &bytes).unwrap();
    env.hush(&["get", "A"]).fails().stderr_has("cannot decrypt");
    env.hush_in(&["set", "B"], "2\n").fails().stderr_has("cannot decrypt");
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes,
        "a failed write must leave the file as it was"
    );
}

#[test]
fn files_moved_between_environments_or_projects_are_rejected() {
    let env = Env::with_project("demo");
    env.hush(&["project", "create", "other"]).ok();
    env.set(&["A"], "dev-value");
    env.set(&["-e", "prod", "A"], "prod-value");
    env.set(&["-p", "other", "A"], "other-value");

    fs::copy(env.file("demo", "prod"), env.file("demo", "dev")).unwrap();
    env.hush(&["get", "A"]).fails().stderr_has("cannot decrypt");
    fs::copy(env.file("other", "dev"), env.file("demo", "dev")).unwrap();
    env.hush(&["get", "A"]).fails().stderr_has("cannot decrypt");
}

#[test]
fn names_are_validated_at_the_boundary() {
    let env = Env::with_project("demo");
    for project in ["../evil", "..", "a/b", "a\\b", "Demo", "con"] {
        env.hush_in(&["-p", project, "set", "A"], "1\n")
            .fails()
            .stderr_has("invalid project name");
        env.hush(&["project", "create", project]).fails();
    }
    for e in ["../x", "nul", "Prod", "a.b"] {
        env.hush_in(&["-e", e, "set", "A"], "1\n")
            .fails()
            .stderr_has("invalid environment name");
    }
    for name in ["A-B", "1A", "A B", "A=B"] {
        env.hush_in(&["set", name], "1\n")
            .fails()
            .stderr_has("invalid secret name");
    }
    env.hush_in(&["set", "A"], "nul\0byte\n").fails().stderr_has("NUL");
    let parent = env.home.path().parent().unwrap();
    assert!(!parent.join("evil").exists() && !env.home.path().join("evil").exists());
    assert!(!env.home.path().join("projects").join("x.hush").exists());
}

#[test]
fn production_environments_need_allow_prod_to_be_read() {
    let env = Env::with_project("demo");
    for prod in ["prod", "production", "prod-eu"] {
        env.set(&["-e", prod, "SECRET"], "live");
        env.hush(&["-e", prod, "get", "SECRET"])
            .fails()
            .stderr_has("--allow-prod");
        env.hush(&["-e", prod, "export"]).fails().stderr_has("--allow-prod");
        env.hush(&["-e", prod, "run", "--", "x"])
            .fails()
            .stderr_has("--allow-prod");
        assert_eq!(env.get(&["-e", prod, "--allow-prod", "SECRET"]), "live");
        // Names only: listing a prod environment needs no flag.
        assert_eq!(env.hush(&["-e", prod, "list"]).ok().stdout(), "SECRET\n");
    }
    let mut c = env.cmd(KEY_A);
    c.env("HUSH_ENV", "prod");
    env.run_with(c, &["get", "SECRET"], None)
        .fails()
        .stderr_has("--allow-prod");
}

#[cfg(windows)]
fn print_var(name: &str) -> Vec<String> {
    vec!["cmd".into(), "/C".into(), format!("echo %{name}%")]
}
#[cfg(not(windows))]
fn print_var(name: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), format!("printf '%s\\n' \"${name}\"")]
}
#[cfg(windows)]
fn exit_with(code: i32) -> Vec<String> {
    vec!["cmd".into(), "/C".into(), format!("exit {code}")]
}
#[cfg(not(windows))]
fn exit_with(code: i32) -> Vec<String> {
    vec!["sh".into(), "-c".into(), format!("exit {code}")]
}

fn run_args(cmd: &[String]) -> Vec<&str> {
    let mut v = vec!["run", "--"];
    v.extend(cmd.iter().map(String::as_str));
    v
}

#[test]
fn run_injects_secrets_and_passes_the_exit_code_through() {
    let env = Env::with_project("demo");
    env.set(&["HUSH_T"], "from-vault");
    let out = env.hush(&run_args(&print_var("HUSH_T"))).ok().stdout();
    assert_eq!(out.trim_end(), "from-vault");

    // The vault value wins over a variable already set in the parent shell.
    let mut c = env.cmd(KEY_A);
    c.env("HUSH_T", "from-parent");
    let out = env.run_with(c, &run_args(&print_var("HUSH_T")), None).ok().stdout();
    assert_eq!(out.trim_end(), "from-vault");

    let r = env.hush(&run_args(&exit_with(7))).fails();
    assert_eq!(r.out.status.code(), Some(7));
    env.hush(&["run", "--", "definitely-not-a-command-zqx"])
        .fails()
        .stderr_has("command not found");
}

#[test]
fn run_without_secrets_warns_and_still_runs() {
    let env = Env::with_project("demo");
    env.hush(&run_args(&exit_with(0)))
        .ok()
        .stderr_has("no secrets stored in demo/dev");
}

#[cfg(windows)]
#[test]
fn run_finds_cmd_shims_on_path_like_npm() {
    let env = Env::with_project("demo");
    env.set(&["HUSH_T"], "via-shim");
    let bin = env.repo.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("probe-zqx.cmd"), "@echo off\r\necho %HUSH_T%\r\n").unwrap();
    let path = format!("{};{}", bin.display(), std::env::var("PATH").unwrap());
    let mut c = env.cmd(KEY_A);
    c.env("PATH", path);
    let out = env.run_with(c, &["run", "--", "probe-zqx"], None).ok().stdout();
    assert_eq!(out.trim_end(), "via-shim");
}

#[test]
fn import_reads_dotenv_rules_and_export_round_trips() {
    let env = Env::with_project("demo");
    let src = env.repo.path().join(".env.local");
    fs::write(
        &src,
        "# comment\nPLAIN=abc\nexport EXPORTED=yes\nSPACED=\"two words\"\nHASH=\"a#b\"\nTRAIL=x # note\n\
         SINGLE='lit$eral'\nESCAPED=\"cost \\$5\"\nMULTI=\"line1\\nline2\"\nEMPTY=\n",
    )
    .unwrap();
    env.hush(&["import", src.to_str().unwrap()])
        .ok()
        .stderr_has("Imported 9 secrets");
    let got = env.json("dev");
    let want: BTreeMap<String, String> = [
        ("PLAIN", "abc"),
        ("EXPORTED", "yes"),
        ("SPACED", "two words"),
        ("HASH", "a#b"),
        ("TRAIL", "x"),
        ("SINGLE", "lit$eral"),
        ("ESCAPED", "cost $5"),
        ("MULTI", "line1\nline2"),
        ("EMPTY", ""),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    for (k, v) in &want {
        assert_eq!(got.get(k), Some(v), "{k}");
    }

    let exported = env.hush(&["export"]).ok().stdout();
    let copy = env.repo.path().join("exported.env");
    fs::write(&copy, &exported).unwrap();
    env.hush(&["-e", "copy", "import", copy.to_str().unwrap()]).ok();
    assert_eq!(env.json("copy"), got);

    env.hush_in(&["set", "PLAIN"], "changed\n").ok();
    env.hush(&["import", src.to_str().unwrap()])
        .ok()
        .stderr_has("0 new, 9 replaced");
}

#[test]
fn a_bad_dotenv_file_imports_nothing() {
    let env = Env::with_project("demo");
    let src = env.repo.path().join("bad.env");
    fs::write(&src, "GOOD=1\nBAD-NAME=2\n").unwrap();
    env.hush(&["import", src.to_str().unwrap()]).fails();
    assert!(!env.file("demo", "dev").exists());
}

#[test]
fn import_errors_never_show_values() {
    let env = Env::with_project("demo");
    let canary = "canary-7f3e1b";
    for (body, line) in [
        (format!("GOOD=1\nDB_PASSWORD=hunter2 {canary} words\n"), 2),
        (format!("# note\n\nA=\"x\\q{canary}\"\n"), 3),
        (format!("A=1\n{canary}\n"), 2),
        (format!("A=1\nB=\"{canary}\nstill open\n"), 2),
        (format!("1BAD={canary}\n"), 1),
    ] {
        let src = env.repo.path().join("bad.env");
        fs::write(&src, &body).unwrap();
        let r = env
            .hush(&["import", src.to_str().unwrap()])
            .fails()
            .stderr_has(&format!("line {line} is not valid dotenv"));
        assert!(
            !r.stderr().contains(canary) && !r.stdout().contains(canary),
            "value leaked for {body:?}: {}",
            r.stderr()
        );
        assert!(!env.file("demo", "dev").exists());
    }
}

#[test]
fn import_refuses_unescaped_dollar_and_writes_nothing() {
    let env = Env::with_project("demo");
    let src = env.repo.path().join(".env");
    for (body, line) in [
        ("A=1\nURL=https://x/?t=${HUSH_KEY}\n", 2),
        ("A=$HUSH_KEY\n", 1),
        ("A=\"multi\nline $HUSH_KEY\"\n", 2),
        ("A='ok $1'\nB=\"\\\\$HUSH_KEY\"\n", 2),
    ] {
        fs::write(&src, body).unwrap();
        let r = env
            .hush(&["import", src.to_str().unwrap()])
            .fails()
            .stderr_has(&format!("line {line} has an unescaped '$'"))
            .stderr_has("single-quote the value or escape it as \\$");
        assert!(!r.stderr().contains("8899aabb"), "{}", r.stderr());
        assert!(!env.file("demo", "dev").exists(), "{body:?} wrote the vault");
    }
    // A '$' in a comment is never expanded, so it is allowed.
    fs::write(&src, "# costs $5\nA=1 # see $HOME\n").unwrap();
    env.hush(&["import", src.to_str().unwrap()])
        .ok()
        .stderr_has("Imported 1 secret into");
}

#[test]
fn a_repeated_key_in_an_import_counts_once_and_the_last_value_wins() {
    let env = Env::with_project("demo");
    let src = env.repo.path().join(".env");
    fs::write(&src, "A=1\nA=2\n").unwrap();
    env.hush(&["import", src.to_str().unwrap()])
        .ok()
        .stderr_has("Imported 1 secret into demo/dev (1 new, 0 replaced).");
    assert_eq!(env.get(&["A"]), "2");
}

#[test]
fn names_that_differ_only_in_case_are_refused() {
    let env = Env::with_project("demo");
    env.set(&["db_url"], "one");
    let before = fs::read(env.file("demo", "dev")).unwrap();

    env.hush_in(&["set", "DB_URL"], "two\n")
        .fails()
        .stderr_has("DB_URL differs from db_url in demo/dev only by case");
    assert_eq!(fs::read(env.file("demo", "dev")).unwrap(), before);

    let src = env.repo.path().join(".env");
    fs::write(&src, "OTHER=x\nDB_URL=two\n").unwrap();
    env.hush(&["import", src.to_str().unwrap()])
        .fails()
        .stderr_has("DB_URL differs from db_url in demo/dev only by case");
    assert_eq!(fs::read(env.file("demo", "dev")).unwrap(), before);

    // Two such names within one file, into an empty environment.
    fs::write(&src, "db_url=1\nDB_URL=2\n").unwrap();
    env.hush(&["-e", "fresh", "import", src.to_str().unwrap()])
        .fails()
        .stderr_has("DB_URL differs from db_url in");
    assert!(!env.file("demo", "fresh").exists());

    // The exact same name is still an update, and another environment is separate.
    env.hush_in(&["set", "db_url"], "three\n")
        .ok()
        .stderr_has("Updated db_url");
    assert_eq!(env.get(&["db_url"]), "three");
    env.set(&["-e", "test", "DB_URL"], "four");
}

#[test]
fn an_empty_value_on_stdin_changes_nothing() {
    let env = Env::with_project("demo");
    env.set(&["A"], "kept");
    let before = fs::read(env.file("demo", "dev")).unwrap();
    for input in ["", "\n", "\r\n"] {
        env.hush_in(&["set", "A"], input).fails().stderr_has("no value");
    }
    assert_eq!(fs::read(env.file("demo", "dev")).unwrap(), before);
    assert_eq!(env.get(&["A"]), "kept");
}

#[test]
fn an_empty_hush_home_or_hush_key_is_an_error() {
    let env = Env::with_project("demo");
    for empty in ["", "   "] {
        let mut c = env.cmd(KEY_A);
        c.env("HUSH_HOME", empty);
        env.run_with(c, &["list"], None)
            .fails()
            .stderr_has("HUSH_HOME is set but empty");
        env.run_with(env.cmd(empty), &["list"], None)
            .fails()
            .stderr_has("HUSH_KEY is set but empty");
    }
}

#[test]
fn run_does_not_pass_the_vault_key_to_the_child() {
    let env = Env::with_project("demo");
    env.set(&["A"], "1");
    let out = env.hush(&run_args(&print_var("HUSH_KEY"))).ok().stdout();
    assert!(!out.contains("8899aabb"), "child saw HUSH_KEY: {out}");
}

#[test]
fn concurrent_writers_lose_nothing() {
    let env = Env::with_project("demo");
    let children: Vec<_> = (0..16)
        .map(|i| {
            let mut c = env.cmd(KEY_A);
            c.args(["set", &format!("K{i:02}")])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            let mut child = c.spawn().unwrap();
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(format!("v{i}\n").as_bytes())
                .unwrap();
            child
        })
        .collect();
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let names = env.hush(&["list"]).ok().stdout();
    assert_eq!(names.lines().count(), 16, "{names}");
    let leftovers: Vec<_> = all_files(&env.home.path().join("projects"))
        .into_iter()
        .filter(|p| !matches!(p.file_name().unwrap().to_str().unwrap(), "dev.hush" | ".lock"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files left behind: {leftovers:?}");
}

#[test]
fn destructive_and_revealing_commands_refuse_without_a_terminal() {
    let env = Env::with_project("demo");
    env.hush_in(&["project", "delete", "demo"], "demo\n")
        .fails()
        .stderr_has("interactive terminal");
    assert!(env.hush(&["project", "list"]).ok().stdout().contains("demo"));
    env.hush(&["recovery-key"]).fails().stderr_has("interactive terminal");
}

#[test]
fn an_agent_shell_cannot_print_values_or_unlock_prod() {
    let env = Env::with_project("demo");
    env.set(&["A"], "1");
    env.set(&["-e", "prod", "A"], "live");
    let agent = |args: &[&str]| {
        let mut c = env.cmd(KEY_A);
        c.env("CLAUDECODE", "1");
        env.run_with(c, args, None)
    };
    for args in [
        &["get", "A"][..],
        &["export"],
        &["export", "--format", "json"],
        &["recovery-key"],
    ] {
        agent(args).fails().stderr_has("AI agent");
    }
    agent(&["-e", "prod", "--allow-prod", "run", "--", "x"])
        .fails()
        .stderr_has("AI agent");
    // Names, writes and dev runs stay available to agents.
    assert_eq!(
        agent(&["list"]).ok().stdout(),
        "A
"
    );
    agent(&run_args(&exit_with(0))).ok();
}

#[test]
fn piped_get_prints_the_value_with_no_trailing_newline() {
    let env = Env::with_project("demo");
    // Compare raw bytes: the value ends in a space, which a trimming helper would hide.
    env.set(&["PLAIN"], "abc123");
    env.set(&["SPACED"], "ends with a space ");
    assert_eq!(env.hush(&["get", "PLAIN"]).ok().out.stdout, b"abc123");
    assert_eq!(env.hush(&["get", "SPACED"]).ok().out.stdout, b"ends with a space ");
}

fn value_strategy() -> impl Strategy<Value = String> {
    // Any non-empty text without NUL (`set` refuses an empty value). A value ending in CR loses
    // it, since stdin input strips one CRLF.
    "[^\u{0}]{1,40}".prop_filter("no trailing CR", |v| !v.ends_with('\r'))
}

#[test]
fn property_set_get_round_trips_any_value() {
    let env = Env::with_project("demo");
    let mut runner = TestRunner::new(Config {
        cases: 40,
        ..Config::default()
    });
    runner
        .run(&value_strategy(), |v| {
            env.hush_in(&["set", "P"], &format!("{v}\n")).ok();
            prop_assert_eq!(env.get(&["P"]), v);
            Ok(())
        })
        .unwrap();
}

#[test]
fn property_export_then_import_round_trips_any_values() {
    let env = Env::with_project("demo");
    let mut runner = TestRunner::new(Config {
        cases: 40,
        ..Config::default()
    });
    let maps = proptest::collection::btree_map("[A-Z_][A-Z0-9_]{0,8}", value_strategy(), 1..5);
    runner
        .run(&maps, |m| {
            for e in ["src", "dst"] {
                let _ = fs::remove_file(env.file("demo", e));
            }
            for (k, v) in &m {
                env.hush_in(&["-e", "src", "set", k], &format!("{v}\n")).ok();
            }
            let exported = env.hush(&["-e", "src", "export"]).ok().stdout();
            let path = env.repo.path().join("roundtrip.env");
            fs::write(&path, exported).unwrap();
            env.hush(&["-e", "dst", "import", path.to_str().unwrap()]).ok();
            prop_assert_eq!(env.json("dst"), m);
            Ok(())
        })
        .unwrap();
}

#[test]
fn import_never_copies_the_environment_into_a_value() {
    // hush's '$' scanner and dotenvy disagree here. The scanner reads `'\'` as a closed quote
    // holding a backslash and the no-break space plus `#` as the start of a comment, so it passes
    // the line. dotenvy's line splitter treats `\'` inside single quotes as an escape and so sees
    // no comment, and its value parser expands `${...}`. The scanner is a friendly early error;
    // what keeps the key out is that import empties its environment before parsing.
    let env = Env::with_project("demo");
    let src = env.repo.path().join(".env");
    fs::write(&src, "A='\\'\u{a0}#${HUSH_KEY}${HUSHTTOKEN}'\\'\n").unwrap();
    let mut c = env.cmd(KEY_A);
    c.env("HUSHTTOKEN", "shell-token-5e2a");
    env.run_with(c, &["import", src.to_str().unwrap()], None).ok();
    let got = env.json("dev");
    let value = &got["A"];
    assert!(
        !value.contains("8899aabb") && !value.contains("shell-token-5e2a"),
        "the environment reached a stored value"
    );
    assert_eq!(value, "\\\u{a0}#\\");
}

#[test]
fn import_accepts_a_utf8_byte_order_mark() {
    let env = Env::with_project("demo");
    let src = env.repo.path().join("bom.env");
    fs::write(&src, b"\xef\xbb\xbfA=1\r\nB=2\r\n").unwrap();
    env.hush(&["import", src.to_str().unwrap()])
        .ok()
        .stderr_has("Imported 2 secrets");
    assert_eq!(
        env.json("dev"),
        BTreeMap::from([("A".into(), "1".into()), ("B".into(), "2".into())])
    );
}

#[test]
fn init_ignores_hush_project_from_the_environment() {
    let env = Env::new();
    let repo = env.repo.path().join("myrepo");
    fs::create_dir_all(&repo).unwrap();
    let mut c = env.cmd(KEY_A);
    c.current_dir(&repo).env("HUSH_PROJECT", "leftover");
    env.run_with(c, &["init"], None)
        .ok()
        .stderr_has("init ignores HUSH_PROJECT")
        .stderr_has("project myrepo");
    let toml = fs::read_to_string(repo.join("hush.toml")).unwrap();
    assert!(toml.contains("project = \"myrepo\""), "{toml}");
    assert!(!env.hush(&["project", "list"]).ok().stdout().contains("leftover"));

    // An explicit -p still names the project, before or after the subcommand.
    for (dir, args) in [
        ("a", &["-p", "named-a", "init"][..]),
        ("b", &["init", "--project", "named-b"]),
    ] {
        let sub = env.repo.path().join(dir);
        fs::create_dir_all(&sub).unwrap();
        let mut c = env.cmd(KEY_A);
        c.current_dir(&sub).env("HUSH_PROJECT", "leftover");
        env.run_with(c, args, None).ok();
        let toml = fs::read_to_string(sub.join("hush.toml")).unwrap();
        assert!(toml.contains(&format!("project = \"named-{dir}\"")), "{toml}");
    }
}

#[test]
fn setup_with_hush_key_refuses_a_vault_whose_vault_toml_is_missing() {
    let env = Env {
        home: TempDir::new().unwrap(),
        repo: TempDir::new().unwrap(),
    };
    let project = env.home.path().join("projects").join("demo");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("dev.hush"), "sealed elsewhere").unwrap();
    env.hush(&["setup"]).fails().stderr_has("vault.toml is missing");
    assert!(!env.home.path().join("vault.toml").exists());
}
