"""Development acceptance cases, mapped to the owner's numbered requirements."""

import json
import os
from pathlib import Path
import shutil
import sys

from acceptance import hush_processes, require, scenario


@scenario("runtime-secret-is-scoped-to-launched-app", 1)
def runtime_scope(s):
    s.linked()
    value = s.value()
    s.put("SESSION_TOKEN", value)
    app = s.python_app({"SESSION_TOKEN": value}, absent=("HUSH_KEY",))
    before = s.command([sys.executable, app])
    require(before.returncode == 91 and before.stdout.strip() == b"APP_FAIL", "plain app unexpectedly received the secret")
    s.app_run({"SESSION_TOKEN": value})
    after = s.command([sys.executable, app])
    require(after.returncode == 91, "secret escaped into a later plain process")
    require("SESSION_TOKEN" not in s.env and "SESSION_TOKEN" not in os.environ, "secret escaped into parent environment")
    s.commit_repo()
    s.scan()


@scenario("vault-at-rest-and-link-file-hold-no-plaintext", 1)
def encrypted_storage(s):
    s.linked("disk-check")
    value = s.value()
    s.put("DISK_CANARY", value)
    require(not s.home.is_relative_to(s.repo), "vault is inside the repo")
    encrypted = s.home / "projects/disk-check/dev.hush"
    require(encrypted.is_file() and encrypted.stat().st_size > 32, "encrypted vault file missing")
    s.ok(s.hush("list"), "list secret names")
    require(s.ok(s.hush("list")).stdout.strip() == b"DISK_CANARY", "list did not return names only")
    require(not list(s.repo.glob(".env*")), "runtime flow wrote a dotenv file")
    config = (s.repo / "hush.toml").read_text(encoding="utf-8")
    fields = [line.strip() for line in config.splitlines() if line.strip() and not line.lstrip().startswith("#")]
    require(fields == ['project = "disk-check"'], "hush.toml holds more than the project link")
    s.commit_repo()
    s.scan()


@scenario("new-project-links-vault-and-returns-both-files", 2)
def new_project_link(s):
    s.setup()
    s.init_git()
    files, _ = s.link_hush("new-web-app")
    require(files == ["hush.toml", ".gitignore"], "link_hush returned the wrong commit file list")
    require((s.repo / "hush.toml").is_file(), "hush.toml was not created")
    require('project = "new-web-app"' in (s.repo / "hush.toml").read_text(), "wrong vault project link")
    for name in (".env", ".env.local", ".env.production", "apps/web/.env.local"):
        require(s.ignored(name), "git does not ignore a dotenv path")
    require(not s.ignored(".env.example"), "example template must stay committable")
    require(s.project_names() == {"new-web-app"}, "vault project was not created")


@scenario("new-project-without-hush-degrades-gracefully", 2)
def no_hush_on_path(s):
    s.setup()
    s.init_git()
    before = s.vault_snapshot()
    files, output = s.link_hush("offline-project", missing=True)
    require(files == [], "missing hush returned files to commit")
    require("not installed" in output and "hush init offline-project" in output, "missing hush did not explain recovery")
    require(not (s.repo / "hush.toml").exists() and not (s.repo / ".gitignore").exists(), "missing hush wrote partial plumbing")
    require(s.vault_snapshot() == before and not s.project_names(), "missing hush changed the vault")


@scenario("new-project-link-preserves-existing-ignore-rules", 2)
def link_preserves_ignore(s):
    s.setup()
    s.init_git()
    (s.repo / ".gitignore").write_text("cache/", encoding="utf-8")
    first, _ = s.link_hush("repeatable")
    before = (s.repo / ".gitignore").read_bytes()
    second, _ = s.link_hush("repeatable")
    require(first == second == ["hush.toml", ".gitignore"], "repeat link returned wrong files")
    require((s.repo / ".gitignore").read_bytes() == before, "repeat link duplicated ignore rules")
    require(s.ignored("cache/file") and s.ignored(".env.local"), "existing or new ignore rules lost")


@scenario("windows-npm-dev-script-routes-through-hush", 3)
def npm_dev(s):
    s.linked("npm-project")
    value = s.value()
    s.put("WEB_API_TOKEN", value)
    s.node_app({"WEB_API_TOKEN": value})
    (s.repo / "package.json").write_text(json.dumps({"private": True, "scripts": {"dev": "hush run -- node app.js"}}), encoding="utf-8")
    s.npm_dev()
    require(not (s.repo / "node_modules").exists(), "npm scenario installed dependencies")


@scenario("windows-hush-run-resolves-npm-cmd", 3)
def hush_launches_npm(s):
    require(os.name == "nt" and s.npm is not None, "Windows npm.cmd is required")
    s.linked()
    value = s.value()
    s.put("BUILD_TOKEN", value)
    s.node_app({"BUILD_TOKEN": value})
    (s.repo / "package.json").write_text(json.dumps({"private": True, "scripts": {"verify": "node app.js"}}), encoding="utf-8")
    result = s.ok(s.hush("run", "--", "npm", "run", "verify"), "hush run npm")
    require(b"APP_OK" in result.stdout.splitlines(), "npm launched by hush did not receive the secret")


@scenario("private-file-stdin-intake-strips-one-crlf", 4)
def private_file_crlf(s):
    s.linked()
    value = s.value()
    path = s.private_file(value, b"\r\n")
    s.route_file("DELIVERED_TOKEN", path)
    path.unlink()
    s.app_run({"DELIVERED_TOKEN": value})


@scenario("private-file-with-no-final-newline-is-exact", 4)
def private_file_no_newline(s):
    s.linked()
    value = s.value() + "  "
    s.values.append(value)
    path = s.private_file(value)
    s.route_file("EXACT_TOKEN", path)
    path.unlink()
    s.app_run({"EXACT_TOKEN": value})


@scenario("piped-stdin-preserves-unicode-and-internal-newline", 4)
def pipe_multiline(s):
    s.linked()
    value = s.value() + "\nsegunda línea λ"
    s.values.append(value)
    s.ok(s.hush("set", "MULTILINE_TOKEN", data=(value + "\n").encode("utf-8")), "piped intake")
    s.app_run({"MULTILINE_TOKEN": value})


@scenario("agent-private-file-set-and-runtime-run-work", 4)
def agent_intake(s):
    s.linked()
    value = s.value()
    path = s.private_file(value)
    s.route_file("AGENT_TOKEN", path, agent=True)
    path.unlink()
    s.app_run({"AGENT_TOKEN": value}, env={"CLAUDECODE": "1"})


@scenario("agent-get-cannot-reveal-a-value", 4)
def agent_get(s):
    s.linked()
    s.put("PRIVATE_TOKEN", s.value())
    result = s.denied(s.hush("get", "PRIVATE_TOKEN", env={"CLAUDECODE": "1"}), "AI agent")
    require(not result.stdout, "agent get wrote output")


@scenario("agent-export-cannot-reveal-values-in-either-format", 4)
def agent_export(s):
    s.linked()
    s.put("PRIVATE_TOKEN", s.value())
    for format_name in ("dotenv", "json"):
        result = s.denied(s.hush("export", "--format", format_name, env={"CLAUDECODE": "1"}), "AI agent")
        require(not result.stdout, "agent export wrote output")


@scenario("agent-allow-prod-cannot-start-an-app", 4)
def agent_allow_prod(s):
    s.linked()
    value = s.value()
    s.put("LIVE_TOKEN", value, environment="prod")
    app = s.python_app({"LIVE_TOKEN": value})
    for environment in ("prod", "dev"):
        result = s.denied(s.hush("--allow-prod", "-e", environment, "run", "--", sys.executable, app,
                                 env={"CLAUDECODE": "1"}), "AI agent")
        require(b"APP_OK" not in result.stdout and b"APP_FAIL" not in result.stdout, "refused command launched the app")


@scenario("project-create-list-is-idempotent-and-isolated", 5)
def project_plumbing(s):
    s.setup()
    require(not s.project_names(), "fresh vault is not empty")
    for name in ("alpha", "beta", "alpha"):
        s.ok(s.hush("project", "create", name), "project create")
    require(s.project_names() == {"alpha", "beta"}, "project create/list disagrees")
    value = s.value()
    s.ok(s.hush("-p", "alpha", "set", "ALPHA_ONLY", data=value.encode()), "explicit-project set")
    require(s.ok(s.hush("-p", "beta", "list")).stdout == b"", "projects share secret names")


@scenario("project-delete-with-piped-name-refuses-without-tty", 5)
def delete_non_tty(s):
    s.linked("keep-me")
    value = s.value()
    s.put("KEEP_TOKEN", value)
    before = s.vault_snapshot()
    s.denied(s.hush("project", "delete", "keep-me", data=b"keep-me\n"), "interactive terminal")
    require(s.vault_snapshot() == before and s.project_names() == {"keep-me"}, "non-TTY delete changed the vault")
    s.app_run({"KEEP_TOKEN": value})


@scenario("native-console-delete-requires-and-accepts-typed-name", 5)
def delete_real_console(s):
    s.linked("console-project")
    s.put("DELETE_TOKEN", s.value())
    before = s.vault_snapshot()
    mismatch = s.console_delete("console-project", "wrong-name")
    require(mismatch.returncode == 2 and mismatch.stdout.strip() == b"NAME_MISMATCH", "wrong typed name was not rejected")
    require(s.vault_snapshot() == before, "wrong-name console delete modified vault")
    result = s.console_delete("console-project", "console-project")
    require(result.returncode == 0 and result.stdout.strip() == b"DELETE_OK", "correct console confirmation did not delete")
    require(not s.project_names() and not (s.home / "projects/console-project").exists(), "deleted project remains on disk")


@scenario("migration-removes-plaintext-from-tree-history-and-vault", 6)
def migrate_local(s):
    s.linked("migrated")
    s.commit_repo()
    expected = {"DB_PASSWORD": s.value(), "API_TOKEN": s.value(), "SIGNING_KEY": s.value()}
    dotenv = s.repo / ".env.local"
    dotenv.write_text("".join(k + "=" + v + "\n" for k, v in expected.items()), encoding="utf-8")
    require(s.ignored(".env.local"), "migration input could be accidentally staged")
    s.ok(s.hush("import", ".env.local"), "hush import")
    dotenv.unlink()
    s.app_run(expected)
    s.commit_repo()
    s.scan()


@scenario("migration-keeps-bom-crlf-and-quoted-values-exact", 6)
def migrate_windows(s):
    s.linked("windows-migration")
    first, second = s.value(), s.value() + " with spaces # literal"
    s.values.append(second)
    dotenv = s.repo / ".env.local"
    dotenv.write_bytes(("\ufeffFIRST='" + first + "'\r\nSECOND=\"" + second + "\"\r\n").encode())
    s.ok(s.hush("import", ".env.local"), "Windows dotenv import")
    dotenv.unlink()
    s.app_run({"FIRST": first, "SECOND": second})
    s.commit_repo()
    s.scan()


@scenario("git-worktree-uses-committed-project-link-and-shared-secrets", 7)
def worktree_shared(s):
    s.linked("shared-repo")
    value = s.value()
    s.put("SHARED_TOKEN", value)
    s.commit_repo()
    worktree = s.root / "feature-checkout"
    s.git("worktree", "add", "-q", "-b", "feature", worktree)
    require((worktree / ".git").is_file(), "fixture is not a real git worktree")
    s.app_run({"SHARED_TOKEN": value}, cwd=worktree)
    updated = s.value()
    s.put("SHARED_TOKEN", updated, cwd=worktree)
    s.app_run({"SHARED_TOKEN": updated})
    require(s.project_names() == {"shared-repo"}, "worktree created another project")


@scenario("monorepo-subfolder-init-gets-an-independent-project", 7)
def monorepo(s):
    s.linked("monorepo-root")
    root_value, app_value = s.value(), s.value()
    s.put("ROOT_ONLY", root_value)
    app = s.repo / "apps/web"
    app.mkdir(parents=True)
    s.ok(s.hush("init", cwd=app), "subfolder init")
    require((app / "hush.toml").is_file() and not (app / ".gitignore").exists(), "subfolder plumbing is misplaced")
    require(s.project_names() == {"monorepo-root", "repo-web"}, "subfolder did not create its own project")
    s.put("APP_ONLY", app_value, cwd=app)
    s.app_run({"APP_ONLY": app_value}, cwd=app, absent=("HUSH_KEY", "ROOT_ONLY"))
    s.app_run({"ROOT_ONLY": root_value}, absent=("HUSH_KEY", "APP_ONLY"))
    require(s.ignored("apps/web/.env.local"), "root ignore does not cover subfolder dotenv")


@scenario("unlinked-repo-does-not-adopt-parent-project", 7)
def repo_boundary(s):
    s.linked("outer")
    s.put("OUTER_ONLY", s.value())
    nested = s.init_git(s.repo / "independent")
    s.denied(s.hush("list", cwd=nested), "no project here")


@scenario("native-commands-need-no-vm-container-or-daemon", 8)
def native_without_services(s):
    baseline = hush_processes()
    s.setup()
    s.init_git()
    # An empty support dir plus the binary is a deliberate PATH allowlist.
    minimal = str(s.binary.parent)
    for command in ("wsl", "wsl.exe", "docker", "docker.exe"):
        require(shutil.which(command, path=minimal) is None, "VM/container tool exists on minimal PATH")
    env = {"PATH": minimal}
    s.ok(s.hush("init", "native-project", env=env), "one-command native project link")
    value = s.value()
    s.ok(s.hush("set", "NATIVE_TOKEN", data=value.encode(), env=env), "native set")
    s.app_run({"NATIVE_TOKEN": value}, env=env)
    s.app_run({"NATIVE_TOKEN": value}, env=env, code=17)
    require(hush_processes() <= baseline, "a new hush process remains after commands returned")


@scenario("dev-is-default-and-vault-overrides-shell-value", 9)
def default_dev(s):
    s.linked()
    dev_value, other_value = s.value(), s.value()
    s.put("ENV_TOKEN", dev_value)
    s.put("ENV_TOKEN", other_value, environment="staging")
    s.app_run({"ENV_TOKEN": dev_value}, env={"ENV_TOKEN": "parent-placeholder"})
    s.app_run({"ENV_TOKEN": other_value}, environment="staging")


@scenario("all-prod-prefix-reads-refuse-without-allow-prod", 9)
def prod_prefixes(s):
    s.linked()
    for environment in ("prod", "production", "prod-eu"):
        s.put("PROD_TOKEN", s.value(), environment=environment)
        for args in (("get", "PROD_TOKEN"), ("export",), ("run", "--", sys.executable, "-c", "print('STARTED')")):
            result = s.denied(s.hush("-e", environment, *args), "--allow-prod")
            require(not result.stdout, "production read produced output or started a child")


@scenario("human-allow-prod-runs-the-selected-production-app", 9)
def human_prod(s):
    s.linked()
    value = s.value()
    s.put("PROD_TOKEN", value, environment="prod")
    s.app_run({"PROD_TOKEN": value}, environment="prod", allow=True)


@scenario("environment-selected-through-shell-is-also-prod-gated", 9)
def env_var_prod(s):
    s.linked()
    s.put("PROD_TOKEN", s.value(), environment="production")
    s.denied(s.hush("run", "--", sys.executable, "-c", "print('STARTED')", env={"HUSH_ENV": "production"}), "--allow-prod")
