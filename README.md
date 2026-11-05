# Hush

Hush keeps every project's secrets in one encrypted vault that lives outside all repos. Each project has its own space in the vault, split into environments such as `dev` and `prod`. Apps get their secrets when they start, through `hush run`, so no file with real values ever sits in a repo.

## Install

```sh
cargo install --path .        # from a checkout of this repo; puts hush in ~/.cargo/bin
```

## First-time setup

```sh
hush setup
hush recovery-key             # save the key it prints in your password manager
```

`hush setup` creates the vault at `%APPDATA%\hush` (macOS: `~/Library/Application Support/hush`) and a random 256-bit key, which it stores in Windows Credential Manager (macOS: Keychain). Without the recovery key, losing the keychain entry means losing every secret, so save it before you store anything.

## Using it in a project

```sh
cd D:\code\my-app
hush init                     # writes hush.toml, creates the project in the vault, adds .env patterns to .gitignore
hush set STRIPE_KEY           # prompts for the value without echoing it
hush import .env.local        # or move an existing .env file in, then delete the file
hush run -- npm run dev       # starts the app with the secrets in its environment
```

Commit `hush.toml`. It only names the vault project (`project = "my-app"`), and it is how every checkout and worktree of the repo finds its secrets. Hush looks for it in the current folder and every folder above it, up to the repo root (the first folder that holds `.git`), so a stray `hush.toml` above the repo is ignored.

Point the scripts that need secrets at `hush run` so nobody has to remember it:

```json
"scripts": {
  "dev": "hush run -- next dev",
  "build": "next build",
  "test": "vitest"
}
```

Only `dev` and scripts that need secrets go through `hush run`. Keep `build`, and tests that need no secrets, as plain commands, so CI, Vercel and cloud sandboxes, which have no vault, can still run them.

Any program works the same way: `hush run -- python main.py`, `hush run -- cargo run`, `hush run -- supabase start`. The app reads its secrets from environment variables as it already does. A value in the vault takes precedence over a variable of the same name already set in the shell. On Windows, `hush run` finds `.cmd` and `.bat` shims on `PATH`, so `npm`, `npx` and `pnpm` work by name. If `HUSH_KEY` is set in the shell, `hush run` removes it from the program's environment, since the program never needs the vault key.

## Commands

| Command | What it does |
|---|---|
| `hush setup [--recover]` | Create the vault and key, or restore the key on a new machine |
| `hush recovery-key` | Show the key's recovery form (interactive terminal only) |
| `hush init [name]` | Link the current repo to a vault project |
| `hush project list` | List projects and the environments that hold secrets |
| `hush project create <name>` | Create an empty project |
| `hush project delete <name>` | Delete a project and its secrets, after you type its name |
| `hush set NAME` | Store a secret. The value comes from a hidden prompt or from stdin, never from the command line |
| `hush get NAME` | Print one value |
| `hush list` | List secret names, never values |
| `hush rm NAME` | Remove a secret |
| `hush import FILE` | Copy every variable in a `.env` file into the vault |
| `hush export [--format dotenv\|json]` | Print every secret, for copying into a hosting platform's settings |
| `hush run -- CMD...` | Run a command with the secrets in its environment |

Every command takes `-p/--project` (default: the project in `hush.toml`) and `-e/--env` (default: `dev`). `HUSH_PROJECT` and `HUSH_ENV` set the same things.

To pipe a value in, for example from a script: `printf '%s' "$VALUE" | hush set NAME`. One trailing newline (or CRLF) is removed. An empty value is refused and the stored secret stays as it was.

`hush import` refuses a file that holds a `$` outside single quotes, unless it is escaped as `\$` or sits in a comment. Dotenv parsers replace `$NAME` and `${NAME}` with variables from the environment, so such a file could copy `HUSH_KEY` or a shell token into a stored value. The error names the line; single-quote the value or escape the `$`, then import again. Nothing is written until the whole file passes. Parse errors name the file and line but never show the line's content, since it may hold a value.

## Environments

Each project has as many environments as you need. `dev` is the default. Give production environments a name that starts with `prod`, such as `prod`, `production` or `prod-eu`: hush refuses to read any environment whose name starts with `prod` (`get`, `export`, `run`) unless you pass `--allow-prod`, so a test or seed script can't pick up production credentials by accident. Listing names and storing values in them needs no flag.

## How secrets are protected

- **At rest.** Each project and environment is one file, `projects/<project>/<env>.hush`, encrypted with XChaCha20-Poly1305 under the vault key. Secret names and values are both inside the encrypted part. Project and environment names are folder and file names, so they are visible.
- **The key** is random and lives in the OS keychain, not next to the vault. Copying the vault folder without the key gives an attacker nothing they can read.
- **Tampering.** A modified file fails to decrypt. Each file is bound to its project and environment, so a file copied over another project's or another environment's file is rejected rather than read.
- **Writes** take a per-project lock and replace the file through a rename, so concurrent `hush set` calls don't lose each other's changes and a crash never leaves a half-written file. A wrong key or a damaged file stops the write before anything changes.
- **Command lines.** `hush set` never takes the value as an argument, so values don't end up in shell history or process listings.

What hush does not protect against:

- Malware running as your Windows user. It can ask the keychain for the key, as hush does.
- The app itself. Once `hush run` starts a program, that program has the values in its environment and can print or log them, as it could with a `.env` file.
- Anyone who reads the output of `hush get` or `hush export`.
- Rollback or deletion of a file. Each environment file is checked on its own, so restoring an older copy of an environment file, or deleting it, is not detected. An older copy decrypts and its old values are used.

## Rules for AI agents

When an agent runs a command, its output goes into the model's context and is sent to the model provider. So agents may use `hush init`, `hush list`, `hush import`, `hush run`, and `hush set` with the value piped in from a file the user provided, but they must never see a value.

Hush enforces part of this itself. When `CLAUDECODE` is set, as it is in every shell Claude Code starts, hush refuses `get`, `export`, `recovery-key` and `--allow-prod`. This guards against accidents, not against an agent working around it: it can't stop `hush run` from starting a program that prints its environment. These rules depend on the agent cooperating. An agent's shell runs as your user, and any process running as your user can read the vault key from Credential Manager (macOS: Keychain) and decrypt the vault itself.

## A new machine

Copy the vault folder (`%APPDATA%\hush`) across, install hush, and run `hush setup --recover`, which asks for the recovery key and puts it in the new machine's keychain.

## Machines without a keychain

On Linux, in CI or in a cloud sandbox, set `HUSH_KEY` to the recovery key and `HUSH_HOME` to the vault folder. While `HUSH_KEY` is set, hush never reads or writes the OS keychain. For hosted apps such as Vercel, the platform keeps its own copy of the production values: run `hush export -e prod --allow-prod` in your own terminal and paste the values into the platform's settings.

## Development

```sh
cargo test                    # unit tests and end-to-end tests against the built binary
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The tests give every case its own vault in a temp folder and a fixed `HUSH_KEY`, so they never touch the real keychain or the real vault.
