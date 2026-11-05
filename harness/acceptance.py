"""Native, offline acceptance tests. Only the Python standard library is used."""

import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import tempfile
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
TARGET = Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target" / "acceptance").resolve()
KEY = "42" * 32
SCENARIOS = []


class Failure(Exception):
    """Only controlled, value-free messages may reach the console."""


def require(condition, message):
    if not condition:
        raise Failure(message)


def scenario(identifier, point):
    def register(function):
        SCENARIOS.append((identifier, point, function))
        return function
    return register


def digest(value):
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def clean_environment():
    # Case-insensitive removal also handles unusual Windows variable casing.
    return {
        k: v for k, v in os.environ.items()
        if k.upper() not in {"CLAUDECODE", "HUSH_PROJECT", "HUSH_ENV", "HUSH_HOME", "HUSH_KEY"}
        and not k.upper().startswith("GIT_")
    }


def hush_processes():
    """Use Toolhelp, not tasklist/PowerShell or a background service."""
    require(os.name == "nt", "this scenario requires native Windows")
    import ctypes
    from ctypes import wintypes

    class Entry(ctypes.Structure):
        _fields_ = [
            ("size", wintypes.DWORD), ("usage", wintypes.DWORD),
            ("pid", wintypes.DWORD), ("heap", ctypes.c_size_t),
            ("module", wintypes.DWORD), ("threads", wintypes.DWORD),
            ("parent", wintypes.DWORD), ("priority", wintypes.LONG),
            ("flags", wintypes.DWORD), ("exe", wintypes.WCHAR * 260),
        ]

    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    api.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    api.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(Entry)]
    api.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(Entry)]
    api.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = api.CreateToolhelp32Snapshot(2, 0)
    require(handle != ctypes.c_void_p(-1).value, "cannot enumerate native processes")
    found = set()
    try:
        entry = Entry()
        entry.size = ctypes.sizeof(entry)
        ok = api.Process32FirstW(handle, ctypes.byref(entry))
        require(ok, "cannot read native process snapshot")
        while ok:
            if entry.exe.lower() == "hush.exe":
                found.add(entry.pid)
            ok = api.Process32NextW(handle, ctypes.byref(entry))
    finally:
        api.CloseHandle(handle)
    return found


class Sandbox:
    def __init__(self, binary, new_project):
        self.temp = tempfile.TemporaryDirectory(prefix="hush-acceptance-")
        self.root = Path(self.temp.name).resolve()
        self.home = self.root / "vault"
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.binary = binary
        self.new_project = new_project
        self.values = []
        self.git_exe = shutil.which("git")
        self.node = shutil.which("node")
        self.npm = shutil.which("npm.cmd" if os.name == "nt" else "npm")
        self.env = clean_environment()
        self.env.update({
            "HUSH_HOME": str(self.home), "HUSH_KEY": KEY,
            "PATH": str(binary.parent) + os.pathsep + self.env.get("PATH", ""),
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_TERMINAL_PROMPT": "0", "PYTHONDONTWRITEBYTECODE": "1",
            "npm_config_offline": "true", "npm_config_audit": "false",
            "npm_config_fund": "false", "npm_config_update_notifier": "false",
            "npm_config_cache": str(self.root / "npm-cache"),
        })

    def close(self):
        self.temp.cleanup()

    def value(self):
        value = "acceptance-" + secrets.token_hex(24)
        self.values.append(value)
        return value

    def command(self, args, *, cwd=None, data=b"", stdin=None, env=None, console=False):
        child_env = self.env.copy()
        if env:
            child_env.update(env)
        # Every subprocess that could launch hush gets these safety settings.
        require(child_env.get("HUSH_HOME") == str(self.home), "unsafe vault override")
        require(child_env.get("HUSH_KEY") == KEY, "unsafe key override")
        options = {}
        if console:
            require(os.name == "nt", "native console unavailable on this platform")
            startup = subprocess.STARTUPINFO()
            startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
            startup.wShowWindow = 0
            options.update(startupinfo=startup, creationflags=subprocess.CREATE_NEW_CONSOLE)
        try:
            result = subprocess.run(
                [str(a) for a in args], cwd=cwd or self.repo, env=child_env,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=45,
                **({"stdin": stdin} if stdin is not None else {"input": data}),
                **options,
            )
        except subprocess.TimeoutExpired:
            raise Failure("command did not exit within 45 seconds") from None
        except OSError:
            raise Failure("could not start required native executable") from None
        self.no_values(result.stdout + result.stderr, "command output")
        return result

    def no_values(self, data, where):
        for value in self.values:
            for encoding in ("utf-8", "utf-16-le", "utf-16-be"):
                require(value.encode(encoding) not in data, "plaintext value found in " + where)

    def ok(self, result, label="command"):
        require(result.returncode == 0, label + " returned a nonzero exit status")
        return result

    def denied(self, result, reason):
        require(result.returncode != 0, "command unexpectedly succeeded")
        require(reason.encode() in result.stderr, "refusal did not explain " + reason)
        return result

    def hush(self, *args, **kwargs):
        return self.command([self.binary, *args], **kwargs)

    def git(self, *args, cwd=None):
        require(self.git_exe is not None, "git is required")
        return self.ok(self.command([self.git_exe, *args], cwd=cwd), "git")

    def init_git(self, path=None):
        path = path or self.repo
        path.mkdir(parents=True, exist_ok=True)
        self.git("init", "-q", "-b", "main", cwd=path)
        return path

    def setup(self):
        self.ok(self.hush("setup"), "hush setup")

    def linked(self, name="acceptance", path=None):
        self.setup()
        self.init_git(path)
        self.ok(self.hush("init", name, cwd=path), "hush init")

    def put(self, name, value, *, environment=None, cwd=None, agent=False):
        args = ["set", name]
        if environment:
            args += ["-e", environment]
        self.ok(self.hush(*args, data=(value + "\n").encode(), cwd=cwd,
                          env={"CLAUDECODE": "1"} if agent else None), "hush set")

    def private_file(self, value, ending=b""):
        path = self.root / ("private-" + secrets.token_hex(4))
        path.write_bytes(value.encode() + ending)
        require(not path.is_relative_to(self.repo), "private file must be outside repo")
        return path

    def route_file(self, name, path, *, agent=False):
        # The actual open file is stdin, exactly the semantics of `hush set NAME < file`.
        with path.open("rb") as source:
            self.ok(self.hush("set", name, stdin=source,
                              env={"CLAUDECODE": "1"} if agent else None), "private-file intake")

    def python_app(self, expected, *, path=None, absent=(), code=0):
        path = path or self.repo / "app.py"
        hashes = {k: digest(v) for k, v in expected.items()}
        path.write_text(
            "import hashlib, os, sys\n"
            + "expected = " + repr(hashes) + "\n"
            + "ok = all(k in os.environ and hashlib.sha256(os.environ[k].encode('utf-8')).hexdigest() == v for k, v in expected.items())\n"
            + "ok = ok and all(k not in os.environ for k in " + repr(tuple(absent)) + ")\n"
            + "print('APP_OK' if ok else 'APP_FAIL')\n"
            + "sys.exit(" + str(code) + " if ok else 91)\n",
            encoding="utf-8",
        )
        return path

    def app_run(self, expected, *, cwd=None, environment=None, env=None, absent=("HUSH_KEY",), code=0, allow=False):
        path = self.python_app(expected, path=(cwd or self.repo) / "app.py", absent=absent, code=code)
        args = ["run"]
        if environment:
            args += ["-e", environment]
        if allow:
            args += ["--allow-prod"]
        result = self.hush(*args, "--", sys.executable, path, cwd=cwd, env=env)
        require(result.returncode == code, "hush run did not return the app's expected exit code")
        require(result.stdout.strip() == b"APP_OK", "app did not confirm all expected hashes")
        return result

    def node_app(self, expected, *, script="app.js"):
        require(self.node is not None, "Node.js is required")
        hashes = {k: digest(v) for k, v in expected.items()}
        (self.repo / script).write_text(
            "const crypto = require('node:crypto');\n"
            + "const expected = " + json.dumps(hashes) + ";\n"
            + "const ok = !('HUSH_KEY' in process.env) && Object.entries(expected).every(([k,v]) => k in process.env && crypto.createHash('sha256').update(process.env[k], 'utf8').digest('hex') === v);\n"
            + "console.log(ok ? 'APP_OK' : 'APP_FAIL'); process.exit(ok ? 0 : 91);\n",
            encoding="utf-8",
        )

    def npm_dev(self):
        require(os.name == "nt", "npm acceptance requires Windows")
        require(self.npm is not None, "npm.cmd is required")
        result = self.ok(self.command([self.npm, "run", "dev"]), "npm run dev")
        require(b"APP_OK" in result.stdout.splitlines(), "npm app did not confirm its hashes")

    def commit_repo(self, cwd=None):
        self.git("add", "--all", cwd=cwd)
        self.git("-c", "user.name=Acceptance", "-c", "user.email=acceptance@example.invalid",
                 "-c", "commit.gpgsign=false", "-c", "core.hooksPath=" + str(self.root / "no-hooks"),
                 "commit", "-qm", "Acceptance fixture", cwd=cwd)

    def scan(self):
        for base, label in ((self.repo, "repo working tree"), (self.home, "vault files")):
            for path in base.rglob("*"):
                if path.is_file() and not (base == self.repo and ".git" in path.relative_to(base).parts):
                    self.no_values(path.read_bytes(), label)
        history = self.git("log", "--all", "-p", "--no-ext-diff").stdout
        self.no_values(history, "git log -p")
        self.no_values((self.repo / "hush.toml").read_bytes(), "hush.toml")

    def vault_snapshot(self):
        return {p.relative_to(self.home).as_posix(): p.read_bytes()
                for p in self.home.rglob("*") if p.is_file()}

    def project_names(self):
        output = self.ok(self.hush("project", "list"), "project list").stdout.decode()
        return {line.split("  (", 1)[0] for line in output.splitlines()}

    def load_new_project(self):
        require(self.new_project.is_file(), "new-project script does not exist")
        spec = importlib.util.spec_from_file_location("acceptance_new_project", self.new_project)
        require(spec is not None and spec.loader is not None, "cannot import new-project script")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        require(callable(getattr(module, "link_hush", None)), "new-project.py has no link_hush function")
        return module

    def new_project_main(self, slug, *, title="Acceptance project", vault_dir=None):
        module = self.load_new_project()
        code = self.root / "code space"
        vault = self.root / "notes"
        goals = vault / "compass" / "goals"
        goals.mkdir(parents=True)
        (goals / "acceptance-goal.md").write_text("# Temporary goal\n", encoding="utf-8")
        repo = code / slug
        t3 = self.root / "stub-t3.cmd"
        calls = []

        def run(args, cwd=None):
            args = list(map(str, args))
            calls.append(args)
            if args[:2] in (["git", "init"], ["git", "add"]):
                require(Path(cwd).resolve() == repo, "project creation escaped its temp repo")
                self.git(*args[1:], cwd=repo)
            elif args[:2] == ["git", "commit"]:
                pass  # Keep the real index staged so the caller can inspect it.
            elif args[:3] == [str(t3), "project", "add"]:
                require(args[3] == repo.as_posix(), "T3 stub received the wrong repo")
            else:
                raise Failure("project creation attempted an unstubbed command")

        argv = [str(self.new_project), slug, "--title", title, "--pitch", "Offline fixture",
                "--goal", "acceptance-goal", "--no-github"]
        if vault_dir:
            argv += ["--vault-dir", vault_dir, "--area", "learning", "--tags", "offline,fixture"]
        captured = io.StringIO()
        with patch.multiple(module, CODE=code, VAULT=vault, GOALS=goals, T3=t3, run=run), \
                patch.object(sys, "argv", argv), patch.dict(os.environ, self.env, clear=True), \
                contextlib.redirect_stdout(captured), contextlib.redirect_stderr(captured):
            try:
                module.main()
            except SystemExit:
                # SystemExit is not an Exception, so it would end the run with no SCORE line.
                raise Failure("new-project main exited early") from None
        self.no_values(captured.getvalue().encode(), "new-project main output")
        require(any(c[:3] == [str(t3), "project", "add"] for c in calls), "T3 stub was not exercised")
        staged = set(self.git("diff", "--cached", "--name-only", cwd=repo).stdout.decode().splitlines())
        return repo, vault / (vault_dir or "projects/" + slug) / "project.md", staged

    def link_hush(self, slug, *, missing=False, path=None):
        linker = self.load_new_project().link_hush
        child_env = self.env.copy()
        if missing:
            empty = self.root / "empty-path"
            empty.mkdir(exist_ok=True)
            child_env["PATH"] = str(empty)
        captured = io.StringIO()
        # Real import + real helper + real binary; only environment and output are scoped.
        with patch.dict(os.environ, child_env, clear=True), contextlib.redirect_stdout(captured), contextlib.redirect_stderr(captured):
            files = linker(path or self.repo, slug)
        self.no_values(captured.getvalue().encode(), "new-project output")
        return files, captured.getvalue()

    def ignored(self, relative, *, cwd=None):
        return self.command([self.git_exe, "check-ignore", "-q", "--no-index", relative], cwd=cwd).returncode == 0

    def console_delete(self, project, typed):
        result = self.command(
            [sys.executable, Path(__file__).with_name("console_delete.py"), self.binary, project, typed],
            console=True,
        )
        require(result.returncode != 77, "Windows console could not be allocated or driven")
        return result

    def console_recovery_key(self):
        result = self.command(
            [sys.executable, Path(__file__).with_name("console_delete.py"), self.binary, "--recovery-key"],
            console=True, env={"CLAUDECODE": "1"},
        )
        require(result.returncode != 77, "Windows recovery console could not be read")
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hush", type=Path, help="use an existing binary instead of building")
    parser.add_argument("--new-project", type=Path, default=Path("D:/code/setup/new-project.py"))
    parser.add_argument("--set", choices=("dev", "test"), default="dev")
    args = parser.parse_args()
    binary_name = "hush.exe" if os.name == "nt" else "hush"
    if args.hush and args.hush.name != binary_name:
        parser.error("--hush binary must be named " + binary_name + " so PATH uses the same executable")
    # Alias prevents duplicate module globals when evals import this script.
    sys.modules["acceptance"] = sys.modules[__name__]
    for path in sorted((ROOT / "harness" / "evals" / args.set).glob("*.py")):
        spec = importlib.util.spec_from_file_location("eval_" + args.set + "_" + path.stem, path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    require(SCENARIOS, "no scenarios found")
    require(len({s[0] for s in SCENARIOS}) == len(SCENARIOS), "duplicate scenario id")
    binary = args.hush.resolve() if args.hush else TARGET / "release" / binary_name
    build_error = None
    if not args.hush:
        build_env = clean_environment()
        build_env["CARGO_TARGET_DIR"] = str(TARGET)
        build_env["CARGO_NET_OFFLINE"] = "true"
        try:
            result = subprocess.run(["cargo", "build", "--release", "--locked", "--offline"],
                                    cwd=ROOT, env=build_env, capture_output=True, timeout=600)
            if result.returncode:
                build_error = "offline release build failed (dependencies/toolchain must be cached)"
        except (OSError, subprocess.TimeoutExpired):
            build_error = "offline release build could not complete"
    if not binary.is_file():
        build_error = build_error or "hush binary does not exist"
    passed = 0
    for identifier, point, function in SCENARIOS:
        box = None
        failure = build_error
        if failure is None:
            try:
                box = Sandbox(binary, args.new_project.resolve())
                function(box)
            except Failure as error:
                failure = str(error)
            except Exception as error:
                # No subprocess output, exception args, or tracebacks: they may contain values.
                failure = "unexpected " + type(error).__name__ + " (details withheld to protect values)"
            finally:
                if box is not None:
                    try:
                        box.close()
                    except OSError:
                        note = "temporary sandbox cleanup failed"
                        failure = failure + "; " + note if failure else note
        if failure:
            print(f"FAIL {identifier}: point {point}: {failure}", flush=True)
        else:
            passed += 1
            print(f"PASS {identifier}: point {point}", flush=True)
    print(f"SCORE: {passed}/{len(SCENARIOS)}", flush=True)
    return 0 if passed == len(SCENARIOS) else 1


if __name__ == "__main__":
    sys.exit(main())
