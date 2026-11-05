# Acceptance baseline, 2026-11-05

Native Windows, Python 3.13, hush source at
`e9ec6086f8f415a45d0585b2282f9fb89074245e`. No hush source or workspace setup
files were changed. Scores below are the final runs after correcting a harness
snapshot path separator issue. Scenario definitions were not adjusted to raise
the score.

| Command | Final line |
| --- | --- |
| `python harness/acceptance.py` (run 1) | `SCORE: 24/27` |
| `python harness/acceptance.py` (run 2) | `SCORE: 24/27` |
| `python harness/acceptance.py --set test` | `SCORE: 11/13` |

Every failure is requirement 2. The default `D:/code/setup/new-project.py` has
no `link_hush` function; its `main` starts at line 20 and its Git staging call
at line 60 stages only README.md. Consequently the real helper cannot be
called for any project-linking scenario:

- `new-project-links-vault-and-returns-both-files`
- `new-project-without-hush-degrades-gracefully`
- `new-project-link-preserves-existing-ignore-rules`
- `test-project-link-keeps-template-and-nested-env-rules`
- `test-unavailable-hush-preserves-existing-project-files`

Supplemental checks used `--hush` pointing to the same built release binary
and `--new-project D:/code/.worktrees/code-hush-routing/setup/new-project.py`.
That routing worktree was at `97d574992b50bb75e19bb81f49c750d392eea079`:

| Set | Final line |
| --- | --- |
| dev | `SCORE: 27/27` |
| test | `SCORE: 13/13` |

Successful deletion was proved through a real hidden Windows console. A
mismatched typed name left the vault unchanged; a correct name removed the
project and all environments while preserving another project's files.
Non-TTY deletion refusal was tested independently. No required scenario was
skipped. T3's actual private delivery UI and the real OS keychain were not
exercised: private file delivery was simulated and every hush invocation used
the isolated test key and vault, as required. Other limits are in README.md.
