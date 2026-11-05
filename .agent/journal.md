# Journal: hush v1

## Goal
Gabriel (2026-11-05): a CLI that holds all secrets per project, encrypted, never in git; apps fetch
them at runtime; new projects know about it and route secrets through it automatically; projects
are separated inside the store. CLI only. Come back with a summary and migration next steps.

reading: away · done · critical · logic
Base: hush main c8b2112 (README only). Branch v1, worktree D:/code/.worktrees/hush-v1.
Second workstream: D:/code (code-layer repo) for new-project + CLAUDE.md + hook integration.

## Decisions
- Rust single binary. RustCrypto XChaCha20-Poly1305 (random 24-byte nonces), AAD = hush/v1/<project>/<env>.
- Random 256-bit vault key in Windows Credential Manager (keyring crate); HUSH_KEY env overrides and
  then the keychain is never touched (tests, CI, Linux). Recovery key = 64 hex in 8 groups, shown only on a TTY.
- Vault at %APPDATA%/hush (HUSH_HOME overrides): projects/<p>/<env>.hush. Names are plaintext, values
  and secret names are encrypted.
- Repo link = committed hush.toml (`project = "x"`), found by walking up from cwd (works in worktrees).
- Environments per project, default dev. prod/production need --allow-prod to read (get/export/run).
- Values never come from argv (shell history, process list): hidden prompt or stdin.
- Writes: per-project lock (std File::lock) + temp file + rename. Wrong key fails before any write.
- Project delete needs a TTY and typing the name; no --yes flag, so agents can't.
- Not built: GUI, sync/sharing with David, cloud backends, Vercel push. Named in the summary.

## Rounds
### Build (2026-11-05)
- Gates: 8 unit + 21 end-to-end tests green (incl. two proptest properties: set/get and export/import
  round-trip of arbitrary text, CR included); clippy -D warnings clean; fmt clean.
- Mutation check, each red as expected: no project lock (concurrency test), no AAD binding (swap test),
  no prod gate, plaintext written to disk, no `$` escape in export (property), no key-id check,
  write after a failed read. (argv-value mutation needed a two-site edit; covered by clap rejecting the arg.)
- Real keychain smoke (temp HUSH_HOME, no HUSH_KEY): setup stores + reads back from Windows Credential
  Manager, init/set work, re-setup verifies, setup after vault removal reuses the key; test credential
  deleted afterwards with cmdkey. Found and fixed: reused-key setup didn't show the recovery reminder.
