# Journal: hush acceptance hillclimb

## Goal
Gabriel (2026-11-05, away): hush fully functioning and proven so. Secrets never stored in git repos;
apps get them at runtime through hush; the secret never lives outside the vault except in the app
that fetched it. Creating a new project in D:\code automatically creates its hush project; every
use of secrets goes through hush; a new secret he gives is routed straight into hush. Project
create/remove plumbing done. No VM (or container, WSL, daemon) needed to plug a project into hush.
Budget: Claude at ~10% weekly; reviews go to Sol this session.

reading: away · metric(100% acceptance scenarios) · critical · logic

## Setup
- Metric: scenarios passing / total, printed by `python harness/acceptance.py` as `SCORE: n/m`.
  Target: 100% on dev and on the held-out test set. Deterministic, so noise floor 0 (checked by two runs).
- Eval set: harness/evals/dev (built from Gabriel's requirements) and harness/evals/test (held out;
  fix workers never read it). Written by Sol, frozen at commit, never edited to raise the score.
- Guardrails every round: cargo fmt/clippy/test green (15 unit + 35 e2e at 10dcb92); no scenario
  touches the real keychain or %APPDATA%\hush (HUSH_KEY + temp HUSH_HOME always set).
- Base: hush v1 10dcb92 (PR #6, Sol-approved; GitHub has not yet picked up the last two pushes).
  code-layer hush-routing 732db72 (PR #1) provides setup/new-project.py link_hush.
- Secret intake design: in T3, the agent calls `request_secret` with file delivery, then
  `hush set NAME < <file>`, so the value never enters the model's context. In a plain terminal,
  Gabriel runs `hush set NAME` himself.

## Rounds

## Open
- Sol builds the eval suite; baseline.
