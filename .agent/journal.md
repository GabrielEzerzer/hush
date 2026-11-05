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
