mod config;
mod crypto;
mod names;
mod run;
mod style;
mod vault;

use std::ffi::OsString;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anstream::{eprint, eprintln, println};
use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use zeroize::Zeroizing;

use crate::names::{check_case_clash, check_path_name, check_secret_name, check_secret_value, is_prod};
use crate::style::{ACCENT, BRAND, DIM, ERR, HEADER, OK, paint};
use crate::vault::{Secrets, Vault};

#[derive(Parser)]
#[command(
    name = "hush",
    version,
    about = "Per-project encrypted secrets, injected into apps at runtime",
    styles = style::clap()
)]
struct Cli {
    #[arg(
        short,
        long,
        global = true,
        env = "HUSH_PROJECT",
        help = "Vault project [default: the `project` in the nearest hush.toml]"
    )]
    project: Option<String>,

    #[arg(
        short,
        long,
        global = true,
        env = "HUSH_ENV",
        default_value = "dev",
        help = "Environment within the project"
    )]
    env: String,

    #[arg(
        long,
        global = true,
        help = "Allow reading an environment whose name starts with prod"
    )]
    allow_prod: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = "Create the vault and its key, stored in the OS keychain")]
    Setup {
        #[arg(
            long,
            help = "Restore the key from its recovery form, for a new machine or a reset keychain"
        )]
        recover: bool,
    },
    #[command(about = "Show the vault key's recovery form (terminal only)")]
    RecoveryKey,
    #[command(about = "Link the current repo to a vault project: writes hush.toml and ignores .env files")]
    Init {
        #[arg(help = "Project name [default: the repo's folder name]")]
        name: Option<String>,
    },
    #[command(subcommand, about = "List, create or delete projects")]
    Project(ProjectCmd),
    #[command(about = "Store a secret, reading the value from a hidden prompt or from stdin")]
    Set { name: String },
    #[command(about = "Print a secret's value")]
    Get { name: String },
    #[command(visible_alias = "ls", about = "List secret names, never values")]
    List,
    #[command(about = "Remove a secret")]
    Rm { name: String },
    #[command(about = "Copy every variable in a .env file into the vault")]
    Import { file: PathBuf },
    #[command(about = "Print every secret, as dotenv lines or JSON")]
    Export {
        #[arg(long, value_enum, default_value = "dotenv")]
        format: Format,
    },
    #[command(about = "Run a command with the secrets added to its environment: hush run -- npm run dev")]
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
}

#[derive(Subcommand)]
enum ProjectCmd {
    #[command(visible_alias = "ls", about = "List projects and the environments that hold secrets")]
    List,
    #[command(about = "Create an empty project")]
    Create { name: String },
    #[command(about = "Delete a project and all its secrets (asks you to type its name)")]
    Delete { name: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Dotenv,
    Json,
}

fn main() -> ExitCode {
    let matches = Cli::command().get_matches();
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    if let Cmd::Init { name } = &cli.cmd
        && matches.value_source("project") == Some(ValueSource::EnvVariable)
    {
        if name.is_none() {
            eprintln!(
                "{}: init ignores HUSH_PROJECT; pass the name to use it: {}",
                paint(BRAND, "hush"),
                paint(ACCENT, "hush init <name>")
            );
        }
        cli.project = None;
    }
    match dispatch(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {e:#}", paint(ERR, "hush:"));
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli) -> Result<ExitCode> {
    let home = vault::home()?;
    match cli.cmd {
        Cmd::Setup { recover } => setup(home, recover)?,
        Cmd::RecoveryKey => {
            refuse_agent("show the recovery key")?;
            ensure!(
                io::stdin().is_terminal() && io::stdout().is_terminal(),
                "the recovery key is shown only in an interactive terminal"
            );
            let vault = Vault::open(home)?;
            println!("{}", *crypto::encode_key(vault.key()));
        }
        Cmd::Init { ref name } => init(&home, name.clone().or(cli.project.clone()))?,
        Cmd::Project(ProjectCmd::List) => {
            let projects = vault::projects(&home)?;
            if projects.is_empty() {
                eprintln!(
                    "No projects yet. Run `{}` in a repo to create one.",
                    paint(ACCENT, "hush init")
                );
            }
            for (name, envs) in projects {
                let envs = if envs.is_empty() {
                    "no secrets yet".to_owned()
                } else {
                    envs.join(", ")
                };
                println!("{}  {}", paint(ACCENT, &name), paint(DIM, format!("({envs})")));
            }
        }
        Cmd::Project(ProjectCmd::Create { ref name }) => {
            if vault::create_project(&home, name)? {
                eprintln!("{} {}.", paint(OK, "Created project"), paint(ACCENT, name));
            } else {
                eprintln!("Project {} already exists.", paint(ACCENT, name));
            }
        }
        Cmd::Project(ProjectCmd::Delete { ref name }) => {
            ensure!(vault::project_exists(&home, name)?, "no project '{name}' in the vault");
            ensure!(
                io::stdin().is_terminal(),
                "deleting a project needs an interactive terminal"
            );
            eprint!(
                "This deletes project {} and every secret in it. Type the project name to confirm: ",
                paint(ACCENT, name)
            );
            let mut typed = String::new();
            io::stdin().read_line(&mut typed)?;
            ensure!(typed.trim() == name, "names did not match; nothing deleted");
            vault::delete_project(&home, name)?;
            eprintln!("{} {}.", paint(OK, "Deleted project"), paint(ACCENT, name));
        }
        Cmd::Set { ref name } => {
            let (vault, project, env) = open(&cli, home)?;
            check_secret_name(name)?;
            let value = read_value(name)?;
            check_secret_value(name, &value)?;
            let replaced = vault.update(&project, &env, |s| {
                check_case_clash(s.keys().map(String::as_str), name, &format!("{project}/{env}"))?;
                Ok(s.insert(name.clone(), value.to_string()).is_some())
            })?;
            eprintln!(
                "{} {} in {}.",
                paint(OK, if replaced { "Updated" } else { "Stored" }),
                paint(ACCENT, name),
                paint(DIM, format!("{project}/{env}"))
            );
        }
        Cmd::Get { ref name } => {
            refuse_agent("print secret values")?;
            let (vault, project, env) = open_for_reading(&cli, home)?;
            let secrets = vault.read(&project, &env)?.unwrap_or_default();
            let value = secrets
                .get(name)
                .with_context(|| format!("no secret {name} in {project}/{env}"))?;
            let mut stdout = io::stdout().lock();
            stdout.write_all(value.as_bytes())?;
            if stdout.is_terminal() {
                stdout.write_all(b"\n")?;
            }
            stdout.flush()?;
        }
        Cmd::List => {
            let (vault, project, env) = open(&cli, home)?;
            let secrets = vault.read(&project, &env)?.unwrap_or_default();
            if secrets.is_empty() {
                eprintln!("No secrets in {}.", paint(DIM, format!("{project}/{env}")));
            }
            for name in secrets.keys() {
                println!("{}", paint(ACCENT, name));
            }
        }
        Cmd::Rm { ref name } => {
            let (vault, project, env) = open(&cli, home)?;
            vault.update(&project, &env, |s| {
                s.remove(name)
                    .map(drop)
                    .with_context(|| format!("no secret {name} in {project}/{env}"))
            })?;
            eprintln!(
                "{} {} from {}.",
                paint(OK, "Removed"),
                paint(ACCENT, name),
                paint(DIM, format!("{project}/{env}"))
            );
        }
        Cmd::Import { ref file } => import(&cli, home, file)?,
        Cmd::Export { format } => {
            refuse_agent("print secret values")?;
            let (vault, project, env) = open_for_reading(&cli, home)?;
            let secrets = vault.read(&project, &env)?.unwrap_or_default();
            let mut out = io::stdout().lock();
            match format {
                Format::Dotenv => {
                    for (k, v) in &secrets {
                        writeln!(out, "{}", *dotenv_line(k, v))?;
                    }
                }
                Format::Json => writeln!(out, "{}", *Zeroizing::new(serde_json::to_string_pretty(&secrets)?))?,
            }
        }
        Cmd::Run { ref command } => {
            let (vault, project, env) = open_for_reading(&cli, home)?;
            let secrets = vault.read(&project, &env)?.unwrap_or_else(|| {
                eprintln!(
                    "{}: no secrets stored in {}; running without any",
                    paint(BRAND, "hush"),
                    paint(DIM, format!("{project}/{env}"))
                );
                Secrets::new()
            });
            drop(vault);
            let code = run::run(command, &secrets)?;
            std::process::exit(code);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn project_name(cli: &Cli) -> Result<String> {
    let name = match &cli.project {
        Some(p) => p.clone(),
        None => {
            let cwd = std::env::current_dir()?;
            config::find(&cwd)?
                .map(|f| f.project)
                .context("no project here: run `hush init` in the repo, or pass --project")?
        }
    };
    check_path_name("project", &name)?;
    Ok(name)
}

fn open(cli: &Cli, home: PathBuf) -> Result<(Vault, String, String)> {
    let project = project_name(cli)?;
    check_path_name("environment", &cli.env)?;
    Ok((Vault::open(home)?, project, cli.env.clone()))
}

fn refuse_agent(what: &str) -> Result<()> {
    if agent_shell() {
        bail!("hush does not {what} inside an AI agent's shell (CLAUDECODE is set); run it in your own terminal");
    }
    Ok(())
}

fn agent_shell() -> bool {
    std::env::var_os("CLAUDECODE").is_some_and(|v| !v.is_empty())
}

fn shows_recovery_key(terminal: bool, agent: bool) -> bool {
    terminal && !agent
}

fn open_for_reading(cli: &Cli, home: PathBuf) -> Result<(Vault, String, String)> {
    if cli.allow_prod {
        refuse_agent("read production secrets")?;
    }
    if is_prod(&cli.env) && !cli.allow_prod {
        bail!("'{}' holds production secrets; pass --allow-prod to read them", cli.env);
    }
    open(cli, home)
}

fn setup(home: PathBuf, recover: bool) -> Result<()> {
    let recovered = if recover {
        ensure!(
            io::stdin().is_terminal(),
            "--recover reads the key from a hidden prompt; run it in a terminal"
        );
        let typed = Zeroizing::new(rpassword::prompt_password("Recovery key: ")?);
        Some(crypto::decode_key(&typed)?)
    } else {
        None
    };
    let out = vault::setup(&home, recovered)?;
    let source = if out.key_from_env {
        "HUSH_KEY"
    } else {
        "the OS keychain"
    };
    if out.created_vault {
        eprintln!(
            "{} at {}, with its key in {source}.",
            paint(OK, "Created the vault"),
            paint(DIM, home.display())
        );
    } else {
        eprintln!(
            "The vault at {} is set up and its key in {source} matches.",
            paint(DIM, home.display())
        );
    }
    if (out.new_key || out.created_vault) && !out.key_from_env {
        if shows_recovery_key(io::stdin().is_terminal() && io::stdout().is_terminal(), agent_shell()) {
            let vault = Vault::open(home)?;
            eprintln!(
                "\n{} Save it in your password manager now. Without it, a lost keychain means lost secrets:\n",
                paint(HEADER, "Recovery key.")
            );
            println!("    {}\n", *crypto::encode_key(vault.key()));
        } else {
            eprintln!(
                "Run `{}` in a terminal and save the key in your password manager. Without it, a lost keychain means lost secrets.",
                paint(ACCENT, "hush recovery-key")
            );
        }
    }
    Ok(())
}

fn init(home: &std::path::Path, name: Option<String>) -> Result<()> {
    vault::require_vault(home)?;
    let cwd = std::env::current_dir()?;
    let root = config::repo_root(&cwd);
    let at_root = match (cwd.canonicalize(), root.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => cwd == root,
    };
    let existing = config::read(&cwd)?;
    let name = match (name, &existing) {
        (Some(n), Some(f)) if n != f.project => {
            bail!("{} already links this folder to project '{}'", config::FILE, f.project)
        }
        (Some(n), _) => n,
        (None, Some(f)) => f.project.clone(),
        (None, None) if at_root => config::default_name(&root),
        (None, None) => {
            let folder = cwd
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            format!("{}-{folder}", config::default_name(&root))
        }
    };
    check_path_name("project", &name).context("pass a project name: hush init <name>")?;
    if existing.is_none() {
        config::write(&cwd, &name)?;
        eprintln!(
            "{} {} for project {}.",
            paint(OK, "Wrote"),
            paint(DIM, cwd.join(config::FILE).display()),
            paint(ACCENT, &name)
        );
    }
    if vault::create_project(home, &name)? {
        eprintln!(
            "{} {} in the vault.",
            paint(OK, "Created project"),
            paint(ACCENT, &name)
        );
    } else {
        eprintln!("{} {}.", paint(OK, "Linked to existing project"), paint(ACCENT, &name));
    }
    if config::ignore_env_files(&root)? {
        eprintln!("Added .env patterns to .gitignore.");
    }
    eprintln!("\n{}", paint(HEADER, "Next:"));
    for (cmd, what) in [
        ("hush set NAME", "store a secret (prompts for the value)"),
        (
            "hush import .env.local",
            "move an existing .env file into the vault, then delete the file",
        ),
        ("hush run -- npm run dev", "start the app with its secrets"),
    ] {
        eprintln!(
            "  {}{} {}",
            paint(ACCENT, cmd),
            " ".repeat(29usize.saturating_sub(cmd.len())),
            paint(DIM, what)
        );
    }
    Ok(())
}

fn read_value(name: &str) -> Result<Zeroizing<String>> {
    if io::stdin().is_terminal() {
        let value = Zeroizing::new(rpassword::prompt_password(format!("Value for {name} (hidden): "))?);
        ensure!(!value.is_empty(), "no value entered; nothing stored");
        return Ok(value);
    }
    let mut buf = Zeroizing::new(Vec::new());
    io::stdin()
        .read_to_end(&mut buf)
        .context("reading the value from stdin")?;
    let text = std::str::from_utf8(&buf).context("the value must be UTF-8 text")?;
    let text = text
        .strip_suffix('\n')
        .map(|t| t.strip_suffix('\r').unwrap_or(t))
        .unwrap_or(text);
    ensure!(!text.is_empty(), "no value on stdin; nothing stored");
    Ok(Zeroizing::new(text.to_owned()))
}

fn import(cli: &Cli, home: PathBuf, file: &std::path::Path) -> Result<()> {
    let (vault, project, env) = open(cli, home)?;
    let raw = Zeroizing::new(std::fs::read(file).with_context(|| format!("reading {}", file.display()))?);
    let text = std::str::from_utf8(&raw).map_err(|_| anyhow!("{} is not UTF-8 text", file.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let entries = dotenv_entries(file, text)?;
    clear_env();
    let mut pairs = std::collections::BTreeMap::new();
    for (line, entry) in entries {
        for item in dotenvy::from_read_iter(entry.as_bytes()) {
            let Ok((k, v)) = item else {
                bail!("{}: line {line} is not valid dotenv", file.display());
            };
            let v = Zeroizing::new(v);
            check_secret_name(&k)?;
            check_secret_value(&k, &v)?;
            check_case_clash(pairs.keys().map(String::as_str), &k, &file.display().to_string())?;
            pairs.insert(k, v);
        }
    }
    let total = pairs.len();
    let replaced = vault.update(&project, &env, |s| {
        let place = format!("{project}/{env}");
        for k in pairs.keys() {
            check_case_clash(s.keys().map(String::as_str), k, &place)?;
        }
        Ok(pairs
            .into_iter()
            .filter(|(k, v)| s.insert(k.clone(), v.to_string()).is_some())
            .count())
    })?;
    eprintln!(
        "{} {total} {} into {} {}.",
        paint(OK, "Imported"),
        if total == 1 { "secret" } else { "secrets" },
        paint(ACCENT, format!("{project}/{env}")),
        paint(DIM, format!("({} new, {replaced} replaced)", total - replaced))
    );
    eprintln!(
        "Delete {} once the app starts through `{}`.",
        paint(DIM, file.display()),
        paint(ACCENT, "hush run")
    );
    Ok(())
}

fn clear_env() {
    for (k, _) in std::env::vars_os() {
        // SAFETY: hush starts no threads before this point; its only thread is the ctrlc handler in `run`.
        unsafe { std::env::remove_var(k) };
    }
}

fn dotenv_entries<'a>(file: &std::path::Path, text: &'a str) -> Result<Vec<(usize, &'a str)>> {
    #[derive(PartialEq)]
    enum Quote {
        None,
        Double,
        Single,
    }
    let (mut quote, mut escaped, mut comment, mut after_space) = (Quote::None, false, false, true);
    let (mut line, mut start, mut start_line) = (1, 0, 1);
    let mut out = Vec::new();
    for (i, c) in text.char_indices() {
        if c == '\n' {
            if quote == Quote::None && !escaped {
                out.push((start_line, &text[start..=i]));
                (start, start_line, comment, after_space) = (i + 1, line + 1, false, true);
            }
            line += 1;
            escaped = false;
            continue;
        }
        if comment {
            continue;
        }
        if escaped {
            (escaped, after_space) = (false, false);
            continue;
        }
        let dollar = match (&quote, c) {
            (Quote::Single, '\'') | (Quote::Double, '"') => {
                quote = Quote::None;
                false
            }
            (Quote::Single, _) => false,
            (Quote::None, '#') if after_space => {
                comment = true;
                false
            }
            (Quote::None, '\'') => {
                quote = Quote::Single;
                false
            }
            (Quote::None, '"') => {
                quote = Quote::Double;
                false
            }
            (_, '\\') => {
                escaped = true;
                false
            }
            (_, c) => c == '$',
        };
        ensure!(
            !dollar,
            "{}: line {line} has an unescaped '$', which dotenv would replace with a variable from the environment; single-quote the value or escape it as \\$. Nothing was imported.",
            file.display()
        );
        after_space = quote == Quote::None && c != '\r' && c.is_whitespace();
    }
    if start < text.len() {
        out.push((start_line, &text[start..]));
    }
    Ok(out)
}

fn dotenv_line(name: &str, value: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(name.len() + value.len() + 4));
    out.push_str(name);
    out.push_str("=\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recovery_key_is_shown_only_to_a_person_at_a_terminal() {
        assert!(shows_recovery_key(true, false));
        assert!(!shows_recovery_key(true, true), "printed into an agent's shell");
        assert!(!shows_recovery_key(false, false));
        assert!(!shows_recovery_key(false, true));
    }

    use proptest::prelude::*;
    use proptest::test_runner::RngSeed;

    fn seeded() -> ProptestConfig {
        ProptestConfig {
            cases: 256,
            rng_seed: RngSeed::Fixed(4),
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    proptest! {
        #![proptest_config(seeded())]

        #[test]
        fn an_exported_dotenv_line_imports_as_the_same_name_and_value(
            name in "[A-Za-z_][A-Za-z0-9_]{0,20}",
            value in "[^\u{0}]{0,40}",
        ) {
            let line = format!("{}\n", *dotenv_line(&name, &value));
            let entries = dotenv_entries(std::path::Path::new("x.env"), &line).unwrap();
            prop_assert_eq!(entries.len(), 1);
            let parsed: Vec<(String, String)> =
                dotenvy::from_read_iter(line.as_bytes()).collect::<Result<_, _>>().unwrap();
            prop_assert_eq!(parsed, vec![(name, value)]);
        }

        #[test]
        fn dotenv_entries_split_a_file_without_losing_or_repeating_bytes(text in "[a-z =#'\"\\\\\n\r]{0,80}") {
            let entries = dotenv_entries(std::path::Path::new("x.env"), &text).unwrap();
            let joined: String = entries.iter().map(|(_, entry)| *entry).collect();
            prop_assert_eq!(&joined, &text);
            for (line, entry) in &entries {
                let start = entry.as_ptr() as usize - text.as_ptr() as usize;
                prop_assert_eq!(*line, 1 + text[..start].matches('\n').count());
            }
        }
    }
}
