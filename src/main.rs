mod config;
mod crypto;
mod names;
mod run;
mod vault;

use std::ffi::OsString;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use zeroize::Zeroizing;

use crate::names::{check_path_name, check_secret_name, check_secret_value, is_prod};
use crate::vault::{Secrets, Vault};

/// Per-project encrypted secrets, injected into apps at runtime.
#[derive(Parser)]
#[command(name = "hush", version)]
struct Cli {
    /// Vault project [default: the `project` in the nearest hush.toml]
    #[arg(short, long, global = true, env = "HUSH_PROJECT")]
    project: Option<String>,

    /// Environment within the project
    #[arg(short, long, global = true, env = "HUSH_ENV", default_value = "dev")]
    env: String,

    /// Allow reading the prod or production environment
    #[arg(long, global = true)]
    allow_prod: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create the vault and its key, stored in the OS keychain
    Setup {
        /// Restore the key from its recovery form, for a new machine or a reset keychain
        #[arg(long)]
        recover: bool,
    },
    /// Show the vault key's recovery form (terminal only)
    RecoveryKey,
    /// Link the current repo to a vault project: writes hush.toml and ignores .env files
    Init {
        /// Project name [default: the repo's folder name]
        name: Option<String>,
    },
    /// List, create or delete projects
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Store a secret, reading the value from a hidden prompt or from stdin
    Set { name: String },
    /// Print a secret's value
    Get { name: String },
    /// List secret names, never values
    #[command(visible_alias = "ls")]
    List,
    /// Remove a secret
    Rm { name: String },
    /// Copy every variable in a .env file into the vault
    Import { file: PathBuf },
    /// Print every secret, as dotenv lines or JSON
    Export {
        #[arg(long, value_enum, default_value = "dotenv")]
        format: Format,
    },
    /// Run a command with the secrets added to its environment: hush run -- npm run dev
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// List projects and the environments that hold secrets
    #[command(visible_alias = "ls")]
    List,
    /// Create an empty project
    Create { name: String },
    /// Delete a project and all its secrets (asks you to type its name)
    Delete { name: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Dotenv,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hush: {e:#}");
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
                eprintln!("No projects yet. Run `hush init` in a repo to create one.");
            }
            for (name, envs) in projects {
                let envs = if envs.is_empty() {
                    "no secrets yet".to_owned()
                } else {
                    envs.join(", ")
                };
                println!("{name}  ({envs})");
            }
        }
        Cmd::Project(ProjectCmd::Create { ref name }) => {
            if vault::create_project(&home, name)? {
                eprintln!("Created project {name}.");
            } else {
                eprintln!("Project {name} already exists.");
            }
        }
        Cmd::Project(ProjectCmd::Delete { ref name }) => {
            ensure!(vault::project_exists(&home, name)?, "no project '{name}' in the vault");
            ensure!(
                io::stdin().is_terminal(),
                "deleting a project needs an interactive terminal"
            );
            eprint!("This deletes project {name} and every secret in it. Type the project name to confirm: ");
            let mut typed = String::new();
            io::stdin().read_line(&mut typed)?;
            ensure!(typed.trim() == name, "names did not match; nothing deleted");
            vault::delete_project(&home, name)?;
            eprintln!("Deleted project {name}.");
        }
        Cmd::Set { ref name } => {
            let (vault, project, env) = open(&cli, home)?;
            check_secret_name(name)?;
            let value = read_value(name)?;
            check_secret_value(name, &value)?;
            let replaced = vault.update(&project, &env, |s| {
                Ok(s.insert(name.clone(), value.to_string()).is_some())
            })?;
            eprintln!(
                "{} {name} in {project}/{env}.",
                if replaced { "Updated" } else { "Stored" }
            );
        }
        Cmd::Get { ref name } => {
            refuse_agent("print secret values")?;
            let (vault, project, env) = open_for_reading(&cli, home)?;
            let secrets = vault.read(&project, &env)?.unwrap_or_default();
            let value = secrets
                .get(name)
                .with_context(|| format!("no secret {name} in {project}/{env}"))?;
            println!("{value}");
        }
        Cmd::List => {
            let (vault, project, env) = open(&cli, home)?;
            let secrets = vault.read(&project, &env)?.unwrap_or_default();
            if secrets.is_empty() {
                eprintln!("No secrets in {project}/{env}.");
            }
            for name in secrets.keys() {
                println!("{name}");
            }
        }
        Cmd::Rm { ref name } => {
            let (vault, project, env) = open(&cli, home)?;
            vault.update(&project, &env, |s| {
                s.remove(name)
                    .map(drop)
                    .with_context(|| format!("no secret {name} in {project}/{env}"))
            })?;
            eprintln!("Removed {name} from {project}/{env}.");
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
                eprintln!("hush: no secrets stored in {project}/{env}; running without any");
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

/// Claude Code sets CLAUDECODE in every shell it starts. Anything printed there goes into the
/// model's context, so hush refuses to print values or unlock prod for it.
fn refuse_agent(what: &str) -> Result<()> {
    if std::env::var_os("CLAUDECODE").is_some_and(|v| !v.is_empty()) {
        bail!("hush does not {what} inside an AI agent's shell (CLAUDECODE is set); run it in your own terminal");
    }
    Ok(())
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
        eprintln!("Created the vault at {}, with its key in {source}.", home.display());
    } else {
        eprintln!(
            "The vault at {} is set up and its key in {source} matches.",
            home.display()
        );
    }
    if (out.new_key || out.created_vault) && !out.key_from_env {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            let vault = Vault::open(home)?;
            eprintln!(
                "\nRecovery key. Save it in your password manager now. Without it, a lost keychain means lost secrets:\n"
            );
            println!("    {}\n", *crypto::encode_key(vault.key()));
        } else {
            eprintln!(
                "Run `hush recovery-key` in a terminal and save the key in your password manager. Without it, a lost keychain means lost secrets."
            );
        }
    }
    Ok(())
}

fn init(home: &std::path::Path, name: Option<String>) -> Result<()> {
    vault::require_vault(home)?;
    let root = config::repo_root(&std::env::current_dir()?);
    let existing = config::read(&root)?;
    let name = match (name, &existing) {
        (Some(n), Some(f)) if n != f.project => {
            bail!("{} already links this repo to project '{}'", config::FILE, f.project)
        }
        (Some(n), _) => n,
        (None, Some(f)) => f.project.clone(),
        (None, None) => config::default_name(&root),
    };
    check_path_name("project", &name).context("pass a project name: hush init <name>")?;
    if existing.is_none() {
        config::write(&root, &name)?;
        eprintln!("Wrote {} for project {name}.", root.join(config::FILE).display());
    }
    if vault::create_project(home, &name)? {
        eprintln!("Created project {name} in the vault.");
    } else {
        eprintln!("Linked to existing project {name}.");
    }
    if config::ignore_env_files(&root)? {
        eprintln!("Added .env patterns to .gitignore.");
    }
    eprintln!(
        "\nNext:\n  hush set NAME                 store a secret (prompts for the value)\n  hush import .env.local        move an existing .env file into the vault, then delete the file\n  hush run -- npm run dev       start the app with its secrets"
    );
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
    Ok(Zeroizing::new(text.to_owned()))
}

fn import(cli: &Cli, home: PathBuf, file: &std::path::Path) -> Result<()> {
    let (vault, project, env) = open(cli, home)?;
    let raw = Zeroizing::new(std::fs::read(file).with_context(|| format!("reading {}", file.display()))?);
    let mut pairs = Vec::new();
    for item in dotenvy::from_read_iter(&raw[..]) {
        let (k, v) = item.with_context(|| format!("parsing {}", file.display()))?;
        check_secret_name(&k)?;
        check_secret_value(&k, &v)?;
        pairs.push((k, Zeroizing::new(v)));
    }
    let total = pairs.len();
    let replaced = vault.update(&project, &env, |s| {
        Ok(pairs
            .into_iter()
            .filter(|(k, v)| s.insert(k.clone(), v.to_string()).is_some())
            .count())
    })?;
    eprintln!(
        "Imported {total} secrets into {project}/{env} ({} new, {replaced} replaced).",
        total - replaced
    );
    if raw.contains(&b'$') {
        eprintln!("The file contains '$'. dotenv expands $NAME references, so check those values with `hush get`.");
    }
    eprintln!("Delete {} once the app starts through `hush run`.", file.display());
    Ok(())
}

/// One dotenv line that dotenvy reads back as the same value.
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
