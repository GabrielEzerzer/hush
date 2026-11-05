"""Frozen second check: different data, process shapes, and combined edge cases."""

import json
import os
import shutil
import sys

from acceptance import hush_processes, require, scenario


@scenario("test-python-batch-job-has-scoped-encrypted-credentials", 1)
def python_batch(s):
    s.linked("batch-job")
    expected = {"QUEUE_PASSWORD": s.value(), "RESULTS_TOKEN": s.value()}
    for name, value in expected.items():
        s.put(name, value)
    job = s.python_app(expected, path=s.repo / "worker.py", absent=("HUSH_KEY",), code=23)
    direct = s.command([sys.executable, job])
    require(direct.returncode == 91, "unwrapped batch job received credentials")
    result = s.hush("run", "--", sys.executable, job)
    require(result.returncode == 23 and result.stdout.strip() == b"APP_OK", "batch job did not get both credentials or preserve exit code")
    require(s.command([sys.executable, job]).returncode == 91, "credentials survived in an unrelated process")
    s.commit_repo()
    s.scan()


@scenario("test-project-link-keeps-template-and-nested-env-rules", 2)
def generated_project(s):
    s.setup()
    repo = s.init_git(s.root / "generated project")
    (repo / ".gitignore").write_text("build/\n*.scratch\n", encoding="utf-8")
    files, _ = s.link_hush("data-tools-2026", path=repo)
    require(files == ["hush.toml", ".gitignore"], "helper returned an incorrect file manifest")
    require('project = "data-tools-2026"' in (repo / "hush.toml").read_text(), "helper linked the wrong slug")
    for name in ("packages/client/.env", "services/api/.env.preview", "build/generated", "tmp.scratch"):
        require(s.ignored(name, cwd=repo), "helper lost an existing rule or missed a nested secret file")
    require(not s.ignored("services/api/.env.example", cwd=repo), "safe example cannot be committed")
    require(s.project_names() == {"data-tools-2026"}, "generated project is absent from vault")


@scenario("test-unavailable-hush-preserves-existing-project-files", 2)
def unavailable_link(s):
    s.setup()
    s.init_git()
    (s.repo / ".gitignore").write_bytes(b"out/\r\n")
    (s.repo / "README.md").write_bytes(b"# Existing project\n")
    before = {p.name: p.read_bytes() for p in s.repo.iterdir() if p.is_file()}
    vault_before = s.vault_snapshot()
    files, output = s.link_hush("later-link", missing=True)
    require(files == [] and "hush init later-link" in output, "missing executable did not return a repair hint")
    require(before == {p.name: p.read_bytes() for p in s.repo.iterdir() if p.is_file()}, "helper modified existing files without hush")
    require(vault_before == s.vault_snapshot() and not s.project_names(), "helper modified vault without hush")


@scenario("test-npm-dev-launches-a-commonjs-task-with-two-secrets", 3)
def npm_commonjs(s):
    s.linked("node-task")
    expected = {"SERVICE_AUTH": s.value(), "CACHE_AUTH": s.value()}
    for name, value in expected.items():
        s.put(name, value)
    (s.repo / "tasks").mkdir()
    s.node_app(expected, script="tasks/check.cjs")
    (s.repo / "package.json").write_text(json.dumps({
        "name": "acceptance-task", "private": True, "type": "module",
        "scripts": {"dev": "hush run -- node tasks/check.cjs"},
    }), encoding="utf-8")
    s.npm_dev()
    require(not (s.repo / "package-lock.json").exists(), "npm required an install step")


@scenario("test-agent-private-file-unicode-intake-and-all-read-guards", 4)
def agent_delivery(s):
    s.linked("agent-delivery")
    value = s.value() + " café 雪  "
    s.values.append(value)
    source = s.private_file(value)
    s.route_file("DELIVERED_AUTH", source, agent=True)
    source.unlink()
    agent = {"CLAUDECODE": "1"}
    s.app_run({"DELIVERED_AUTH": value}, env=agent)
    for args in (("get", "DELIVERED_AUTH"), ("export", "--format", "json"),
                 ("--allow-prod", "run", "--", sys.executable, "-c", "print('STARTED')")):
        result = s.denied(s.hush(*args, env=agent), "AI agent")
        require(not result.stdout, "agent guard allowed output or app launch")


@scenario("test-pipe-keeps-internal-crlf-and-empty-update-keeps-vault", 4)
def piped_record(s):
    s.linked()
    value = s.value() + "\r\nrecord-two\r\n"
    s.values.append(value)
    s.ok(s.hush("set", "RECORD_TOKEN", data=(value + "\r\n").encode()), "piped multiline set")
    s.app_run({"RECORD_TOKEN": value})
    before = s.vault_snapshot()
    s.denied(s.hush("set", "RECORD_TOKEN", data=b"\r\n"), "no value")
    require(s.vault_snapshot() == before, "empty update changed stored credentials")
    s.app_run({"RECORD_TOKEN": value})


@scenario("test-console-delete-removes-all-envs-and-keeps-other-project", 5)
def project_delete(s):
    s.linked("remove-this")
    s.put("TOKEN", s.value())
    s.put("TOKEN", s.value(), environment="staging")
    s.ok(s.hush("project", "create", "retain-this"), "create retained project")
    other = s.value()
    s.ok(s.hush("-p", "retain-this", "set", "OTHER_TOKEN", data=other.encode()))
    require(s.project_names() == {"remove-this", "retain-this"}, "create/list failed")
    before = s.vault_snapshot()
    s.denied(s.hush("project", "delete", "remove-this", data=b"remove-this\r\n"), "interactive terminal")
    require(before == s.vault_snapshot(), "piped confirmation changed vault")
    deletion = s.console_delete("remove-this", "remove-this")
    require(deletion.returncode == 0, "console confirmation did not delete project")
    require(s.project_names() == {"retain-this"}, "delete affected the wrong projects")
    remaining = {k: v for k, v in before.items() if not k.startswith("projects/remove-this/")}
    require(s.vault_snapshot() == remaining, "delete failed to remove all environments or changed retained data")


@scenario("test-migration-bom-duplicates-and-literal-dollar-survive", 6)
def migrate_existing(s):
    s.linked("legacy-service")
    s.commit_repo()
    password, webhook, replaced = s.value(), s.value() + "$literal#tail", s.value()
    s.values.append(webhook)
    dotenv = s.repo / ".env.local"
    dotenv.write_bytes(("\ufeff# migrated settings\r\nPASSWORD='" + replaced
                        + "'\r\nWEBHOOK='" + webhook + "'\r\nPASSWORD='" + password + "'\r\n").encode())
    s.ok(s.hush("import", ".env.local"), "migration import")
    dotenv.unlink()
    s.app_run({"PASSWORD": password, "WEBHOOK": webhook})
    s.commit_repo()
    s.scan()


@scenario("test-worktree-descendant-finds-same-link-and-updated-secret", 7)
def nested_worktree(s):
    s.linked("shared-tool")
    value = s.value()
    s.put("TOOL_AUTH", value)
    s.commit_repo()
    checkout = s.root / "alternate tree"
    s.git("worktree", "add", "-q", "--detach", checkout)
    descendant = checkout / "tools/cli/deep"
    descendant.mkdir(parents=True)
    s.app_run({"TOOL_AUTH": value}, cwd=descendant)
    next_value = s.value()
    s.put("TOOL_AUTH", next_value)
    s.app_run({"TOOL_AUTH": next_value}, cwd=descendant)
    require(s.project_names() == {"shared-tool"}, "worktree inferred another vault project")


@scenario("test-monorepo-sibling-default-projects-stay-separate", 7)
def sibling_projects(s):
    s.linked("workspace")
    expected_names = {"workspace"}
    for folder, token in (("admin", "ADMIN_AUTH"), ("jobs", "JOBS_AUTH")):
        app = s.repo / "packages" / folder
        app.mkdir(parents=True)
        s.ok(s.hush("init", cwd=app, env={"HUSH_PROJECT": "unrelated-shell-project"}), "subproject init")
        expected_names.add("repo-" + folder)
        value = s.value()
        s.put(token, value, cwd=app)
        descendant = app / "src"
        descendant.mkdir()
        other = "JOBS_AUTH" if token == "ADMIN_AUTH" else "ADMIN_AUTH"
        s.app_run({token: value}, cwd=descendant, absent=("HUSH_KEY", other))
    require(s.project_names() == expected_names, "sibling projects or stale HUSH_PROJECT linked incorrectly")


@scenario("test-native-minimal-path-needs-no-service-even-on-child-failure", 8)
def no_service(s):
    baseline = hush_processes()
    s.setup()
    s.init_git()
    empty = s.root / "support bin"
    empty.mkdir()
    minimal = str(s.binary.parent) + os.pathsep + str(empty)
    for tool in ("docker.exe", "wsl.exe", "podman.exe"):
        require(shutil.which(tool, path=minimal) is None, "container executable present in allowlisted PATH")
    env = {"PATH": minimal}
    s.ok(s.hush("init", "native-batch", env=env), "native init")
    value = s.value()
    s.ok(s.hush("set", "NATIVE_AUTH", data=value.encode(), env=env))
    s.app_run({"NATIVE_AUTH": value}, code=29, env=env)
    s.denied(s.hush("run", "--", "missing-native-tool-acceptance", env=env), "command not found")
    require(hush_processes() <= baseline, "hush left a daemon after app failure")


@scenario("test-default-dev-explicit-env-and-shell-prod-prefix-guard", 9)
def environment_precedence(s):
    s.linked()
    values = {name: s.value() for name in ("dev", "preview", "prod2")}
    for name, value in values.items():
        s.put("SCOPED_AUTH", value, environment=name)
    s.app_run({"SCOPED_AUTH": values["dev"]})
    s.app_run({"SCOPED_AUTH": values["preview"]}, environment="preview", env={"HUSH_ENV": "prod2"})
    for args in (("get", "SCOPED_AUTH"), ("export",),
                 ("run", "--", sys.executable, "-c", "print('STARTED')")):
        result = s.denied(s.hush(*args, env={"HUSH_ENV": "prod2"}), "--allow-prod")
        require(not result.stdout, "shell-selected production environment was readable")


@scenario("test-production-region-flag-unlocks-only-for-human-shell", 9)
def production_region(s):
    s.linked("regional-job")
    value = s.value()
    s.put("REGION_AUTH", value, environment="prod_west")
    app = s.python_app({"REGION_AUTH": value})
    s.denied(s.hush("-e", "prod_west", "run", "--", sys.executable, app), "--allow-prod")
    s.app_run({"REGION_AUTH": value}, environment="prod_west", allow=True)
    result = s.denied(s.hush("-e", "prod_west", "--allow-prod", "run", "--", sys.executable, app,
                             env={"CLAUDECODE": "1"}), "AI agent")
    require(not result.stdout, "agent launched production app")
