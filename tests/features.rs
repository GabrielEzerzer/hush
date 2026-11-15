//! Keeps features.json complete: every command and flag of the hush binary is mapped, every mapped command
//! answers --help, and every mapped test exists.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const HELP_TIMEOUT: Duration = Duration::from_secs(20);
const FLAGS_WITH_VALUES: &[&str] = &["-p", "--project", "-e", "--env", "--format"];
const FIELDS: &[&str] = &["name", "description", "command", "inputs", "outputs", "test"];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn features() -> Vec<Value> {
    let text = fs::read_to_string(root().join("features.json")).expect("features.json at the repo root");
    serde_json::from_str::<Value>(&text)
        .expect("features.json is valid JSON")
        .as_array()
        .expect("features.json is a list of entries")
        .clone()
}

fn help(args: &[&str]) -> String {
    let home = tempfile::tempdir().unwrap();
    let out_path = home.path().join("help.txt");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hush"))
        .args(args)
        .arg("--help")
        .env("HUSH_HOME", home.path().join("vault"))
        .env_remove("HUSH_PROJECT")
        .env_remove("HUSH_ENV")
        .env_remove("HUSH_KEY")
        .env_remove("CLAUDECODE")
        .stdin(Stdio::null())
        .stdout(fs::File::create(&out_path).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .expect("the hush binary starts");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > HELP_TIMEOUT {
            let _ = child.kill();
            panic!(
                "`hush {} --help` did not finish within {HELP_TIMEOUT:?}",
                args.join(" ")
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "`hush {} --help` exited with {status}",
        args.join(" ")
    );
    fs::read_to_string(out_path).unwrap()
}

fn section<'a>(help: &'a str, title: &str) -> Vec<&'a str> {
    help.lines()
        .skip_while(|line| *line != title)
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .collect()
}

fn subcommands(help: &str) -> Vec<String> {
    section(help, "Commands:")
        .iter()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(str::to_owned)
        .collect()
}

fn long_flags(help: &str) -> Vec<String> {
    section(help, "Options:")
        .iter()
        .flat_map(|line| line.split([' ', ',']))
        .filter(|word| word.starts_with("--") && *word != "--help" && *word != "--version")
        .map(str::to_owned)
        .collect()
}

fn command_paths() -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for name in subcommands(&help(&[])) {
        let children = subcommands(&help(&[&name]));
        if children.is_empty() {
            paths.push(vec![name]);
        } else {
            paths.extend(children.into_iter().map(|child| vec![name.clone(), child]));
        }
    }
    paths
}

fn is_placeholder(word: &str) -> bool {
    word.contains(['<', '[', '.']) || word.chars().any(|c| c.is_ascii_uppercase()) && !word.starts_with('-')
}

struct Mapped {
    help_args: Vec<String>,
    path: Vec<String>,
}

fn parse(command: &str) -> Mapped {
    let mut words = command.split_whitespace().skip_while(|w| w.contains('='));
    assert_eq!(words.next(), Some("hush"), "`{command}` must run the hush binary");
    let (mut help_args, mut path) = (Vec::new(), Vec::new());
    let mut takes_value = false;
    for word in words {
        if takes_value {
            help_args.push(word.to_owned());
            takes_value = false;
        } else if word == "--" || is_placeholder(word) {
            break;
        } else if word.starts_with('-') {
            takes_value = FLAGS_WITH_VALUES.contains(&word);
            help_args.push(word.to_owned());
        } else {
            help_args.push(word.to_owned());
            path.push(word.to_owned());
        }
    }
    Mapped { help_args, path }
}

#[test]
fn every_entry_is_complete_and_its_test_exists() {
    let features = features();
    assert!(!features.is_empty());
    for entry in &features {
        let name = entry["name"].as_str().unwrap_or("<unnamed>");
        for field in FIELDS {
            assert!(
                entry.get(field).is_some(),
                "features.json entry '{name}' has no '{field}'"
            );
        }
        let test = entry["test"].as_str().expect("test is a string");
        if test == "none" {
            assert!(
                entry["issue"].as_u64().is_some_and(|n| n > 0),
                "features.json entry '{name}' has no test, so it needs \"issue\": the number of its debt issue"
            );
            continue;
        }
        let (file, function) = test
            .split_once("::")
            .unwrap_or_else(|| panic!("entry '{name}': test '{test}' must be <file>::<test function>"));
        let source = fs::read_to_string(root().join(file))
            .unwrap_or_else(|_| panic!("entry '{name}': the test file {file} does not exist"));
        assert!(
            source.contains(&format!("fn {function}(")) || source.contains(&format!("def {function}(")),
            "entry '{name}': {file} has no test function {function}"
        );
    }
}

#[test]
fn every_command_and_flag_is_mapped_and_every_mapped_command_answers_help() {
    let features = features();
    let commands: Vec<&str> = features
        .iter()
        .map(|e| e["command"].as_str().expect("command is a string"))
        .collect();
    let mapped: Vec<Mapped> = commands.iter().map(|c| parse(c)).collect();
    for (command, m) in commands.iter().zip(&mapped) {
        let args: Vec<&str> = m.help_args.iter().map(String::as_str).collect();
        help(&args);
        assert!(!m.path.is_empty(), "`{command}` names no hush subcommand");
    }
    let known: BTreeSet<&Vec<String>> = mapped.iter().map(|m| &m.path).collect();
    for path in command_paths() {
        assert!(
            known.contains(&path),
            "`hush {}` is not in features.json; add an entry with its name, description, command, inputs, outputs and test",
            path.join(" ")
        );
        let args: Vec<&str> = path.iter().map(String::as_str).collect();
        for flag in long_flags(&help(&args)) {
            assert!(
                commands.iter().any(|c| c.split_whitespace().any(|w| w == flag)),
                "`hush {} {flag}` is not in features.json; add it to an entry's command",
                path.join(" ")
            );
        }
    }
}

#[test]
fn parse_reads_flags_values_and_placeholders() {
    let m = parse("CLAUDECODE=1 hush --project NAME --env prod get NAME");
    assert_eq!(m.help_args, ["--project", "NAME", "--env", "prod", "get"]);
    assert_eq!(m.path, ["get"]);
    assert_eq!(parse("hush project delete <name>").path, ["project", "delete"]);
    assert_eq!(parse("hush run -- CMD...").help_args, ["run"]);
    assert_eq!(parse("hush init [name]").path, ["init"]);
}
