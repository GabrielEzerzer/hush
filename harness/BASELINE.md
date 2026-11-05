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

## Review additions, 2026-11-05

Base: `1fd556cd300992ccda97f02346d920d8dc109abe` on `acceptance` (pulled,
clean before editing). These changes add coverage and sharpen isolation; no
existing behavioral assertion was removed or weakened. Source, Cargo files,
and the real new-project script were not changed in this checkout.

1. **"Requirement 2 only tests `link_hush` on its own."** Added a dev `main`
   scenario and a different held-out `main` scenario using custom metadata and
   a note path with spaces. Both use `--no-github`, temporary CODE/VAULT/GOALS,
   a temporary goal note, and a command stub. Only Git init/add execute; commit
   is intercepted to inspect the real index, and T3 is stubbed. Both assert the
   project exists in the temporary hush vault and hush.toml/.gitignore are staged.
2. **"The build folder is hard-coded"**: the default is now
   `<checkout>/target/acceptance`, with inherited `CARGO_TARGET_DIR` honored.
   All builds remain locked and offline; build failures still fail every case.
3. **"`--hush` with a binary not named `hush.exe`"**: argument parsing now
   rejects a supplied binary not named `hush.exe` (`hush` off Windows), before
   scenarios run, with a clear PATH-consistency explanation.
4. **"A cleanup failure replaces a scenario's real failure reason"**: append
   the cleanup note to an existing failure, and still fail on cleanup alone.
   Focused mocked runs verified both outcomes without leaving temp folders.
5. **"The 'no VM or container tool' assertions can't fail"**: retained the
   checks and relabeled them as fixture preconditions in code and README.
   Both native scenarios now also run from a Git descendant, preserving root
   runs, application hash/exit checks, missing-tool refusal, and process checks.
6. **"The agent guard on `recovery-key` is untested."** Added a dev scenario
   with `CLAUDECODE=1` using the hidden console helper. Hush gets actual console
   stdin AND stdout so its independent TTY guard cannot mask a missing agent
   guard. The helper checks the screen and stderr for the fixed test key,
   including its hyphen-grouped recovery form, requires the AI-agent refusal,
   and only emits sanitized markers. The parent also checks vault immutability.
7. **"The dev set never runs hush below the folder holding hush.toml."** Added
   descendant worktree runs before and after updating the shared secret, while
   retaining the existing root and worktree assertions.
8. **"Two realistic edges are missing"**: added dev cases for **"`link_hush`
   when hush is installed but `hush setup` never ran"** (empty manifest, setup
   and retry hints, no partial files or vault writes) and **"`hush set NAME
   VALUE` (value on the command line) must be refused"** (argument rejection,
   no vault changes or stored names). The positional argument is a public
   marker, because clap may echo rejected arguments; no generated value is
   placed on a command line or printed.

The first mutation attempt already rejected both skipped-link cases and the
missing recovery guard. Its stronger diagnostic expectation exposed that the
screen detector needed to normalize recovery-key hyphens; this was corrected
before repeating the final suites and mutation checks. Both temporary mutation
worktrees use their own `CARGO_TARGET_DIR` and are removed after the check.

Final validation used
`--new-project D:/code/.worktrees/code-hush-routing/setup/new-project.py`
(routing worktree at `7ca4f7ab7ac7bec5a2c80aa7fba88f16c8e195e7`). All three
commands built offline under this checkout's ignored `target/acceptance`.

| Command (with the explicit new-project argument above) | Final line |
| --- | --- |
| `python harness/acceptance.py` (final run 1) | `SCORE: 31/31` |
| `python harness/acceptance.py` (final run 2) | `SCORE: 31/31` |
| `python harness/acceptance.py --set test` | `SCORE: 14/14` |

Scratch mutation checks copied the final harness into a detached Git worktree
under `%TEMP%`, built with its own `CARGO_TARGET_DIR=<scratch>/target/mutation-checks`,
and passed all three positive controls first. Then:

- Replacing `run(["git", "add", "README.md", *link_hush(repo, a.slug)], cwd=repo)`
  with `run(["git", "add", "README.md"], cwd=repo)` in a scratch new-project.py
  made both `new_project_entrypoint` (dev) and `main_custom_note` (test) fail:
  the temporary hush vault project was missing. Both scenarios independently
  assert the staged plumbing when run against the intact script.
- Removing only `refuse_agent("show the recovery key")?;` from scratch
  `src/main.rs` and rebuilding made `agent_recovery_key` fail. Its hidden
  console helper returned `RECOVERY_KEY_EXPOSED`, proving the remaining TTY
  check passed and the test detected the key without forwarding it.
- Final result: `MUTATION CHECKS: 3/3 rejected; positive controls 3/3 passed`.
  The scratch worktree, source mutation, script copy, and build directory were
  removed. This checkout's source and Cargo files remain unchanged.

Focused runner checks also passed: alternate binary name rejected with exit 2;
inherited relative `CARGO_TARGET_DIR` resolved correctly; cleanup failure
preserved an original scenario reason and still failed an otherwise passing
scenario. Harness Python syntax and `git diff --check` passed.

No required scenario was skipped. Pre-existing `%TEMP%/hush-acceptance-*`
folders were not removed: automatic approval review rejected the cleanup
command with "blocked by policy", despite the process check showing no hush
suite running before the attempt. Suite-created sandboxes and both scratch
mutation worktrees cleaned up successfully. Real keychain, personal vault,
GitHub/T3 actions, network scenarios, and long-running services were not used.
