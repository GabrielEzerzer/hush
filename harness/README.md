# Hush acceptance suite

Run from the hush checkout with Python 3 on native Windows. The suite uses only
the Python standard library; Git, the Rust toolchain, Node.js, and npm must be
installed. No npm install, network access, VM, container, server, or daemon is
needed for scenarios.

```powershell
python harness/acceptance.py
python harness/acceptance.py
python harness/acceptance.py --set test
```

Each invocation builds with `cargo build --release --locked --offline` and
`CARGO_TARGET_DIR=<checkout>/target/acceptance`, or the inherited
`CARGO_TARGET_DIR` when set (relative paths resolve from the invoking directory). Rust
dependencies must already be cached. A build failure fails every scenario;
the runner never silently uses a stale binary. To use an existing executable:

```powershell
python harness/acceptance.py --hush C:/path/to/hush.exe
```

The supplied binary must be named `hush.exe` (`hush` off Windows), so npm and
executable discovery on PATH select the same binary as direct calls.

The default project creation script is `D:/code/setup/new-project.py`. Point at
a candidate version explicitly to test its real `link_hush` function:

```powershell
python harness/acceptance.py --new-project D:/code/.worktrees/code-hush-routing/setup/new-project.py
python harness/acceptance.py --set test --new-project D:/code/.worktrees/code-hush-routing/setup/new-project.py
```

The helper is imported with `importlib` and called on a fresh temporary Git
repository. Its subprocess and executable discovery are real, including the
case where hush is absent from PATH. A missing script or missing `link_hush`
is an acceptance failure, never a skip or a fallback to a different script.
Additional cases run `main` with `--no-github`, temporary CODE/VAULT/GOALS paths,
and a temporary goal note. The `run` stub allows real Git init/add in that repo,
leaves the index staged instead of committing, and intercepts T3. It refuses
other commands. Both sets assert that the hush vault project exists and both
link files are staged; the held-out case uses custom note metadata and a note
path containing spaces. No real GitHub, T3, or personal-vault action runs.

There are 31 development scenarios and 14 held-out scenarios. Each scenario
reports `PASS id: point N` or `FAIL id: point N: reason`; the last line is
`SCORE: n/m`. Exit status is zero only for a full score. The sets use different
data and app shapes; they share only the execution and isolation helpers.
Freeze both eval directories when beginning implementation work. Fix workers
should read the development set only.

Every scenario has a fresh temporary vault and repo. Every command that can
invoke hush receives that vault as `HUSH_HOME` and a fixed 64-hex test key as
`HUSH_KEY`. `CLAUDECODE`, `HUSH_PROJECT`, and `HUSH_ENV` are removed from the
inherited environment and added only by scenarios that exercise them. Git's
inherited routing and global configuration are isolated too. The real
Credential Manager and user vault are never needed. Temporary files and repos
are removed even after a scenario fails.
If cleanup itself fails, its note is appended to the original scenario failure.

Fake values are generated in memory and sent through stdin or private files
outside the repo. App files contain hashes only. Apps compare hashes and print
pass/fail markers; command output is captured, checked for plaintext, and never
echoed. Exception details and build output are withheld because they might
contain values. Migration cases deliberately create `.env.local` only in a
temporary repo, import and delete it, then scan the working tree, `git log -p`,
the link file, and vault files for plaintext in UTF-8 and UTF-16.

`console_delete.py` is launched in a hidden, native Windows console. It writes
project-name keystrokes through `WriteConsoleInputW` and passes a real `CONIN$`
handle as hush's stdin. The parent independently checks project listings and
vault bytes. Console failures count as failures. Process snapshots use the
native Toolhelp API to detect newly surviving hush processes without touching
other users' or existing hush processes.

Recovery-key mode gives hush real console input and output with `CLAUDECODE=1`.
It reads the hidden console screen and checks stderr for the fixed test key,
requires the AI-agent refusal, and emits only a value-free marker. This makes
the test sensitive to removal of the agent guard even though a TTY guard remains.
The dev set also covers installed hush before setup, positional-value refusal,
and secret lookup from a descendant of a linked worktree.

Limits: disk and history scans prove the fake values are absent after the
workflow; they do not inspect RAM, paging, crash dumps, OS telemetry, or recover
secrets already committed before migration. Removing committed secrets would
require a separate history rewrite. Absence of VM/container tools on the minimal
PATH is a fixture precondition, not a behavioral assertion about hush. The
behavioral checks run hush in a Git repo and a descendant with that PATH,
verify application hashes and exit status, and check for surviving hush
processes. They do not uninstall tools from the machine.
