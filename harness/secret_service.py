# Stores, reuses and recovers the vault key through a real Secret Service, with HUSH_KEY unset.
# Run it in a throwaway D-Bus session with an unlocked keyring; it refuses to start if a hush key is already stored.

import os
from pathlib import Path
import pty
import secrets
import select
import subprocess
import sys
import tempfile
import time

ITEM = ["service", "hush", "username", "vault-key"]


def fail(message, output=""):
    print("FAIL: " + message)
    if output:
        print(output)
    sys.exit(1)


def stored_key():
    result = subprocess.run(["secret-tool", "lookup", *ITEM], capture_output=True, text=True)
    return result.stdout.strip() if result.returncode == 0 else None


def hush(binary, env, *args, data=""):
    return subprocess.run([str(binary), *args], input=data, capture_output=True, text=True, env=env, timeout=60)


def in_terminal(binary, env, args, typed):
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(str(binary), [str(binary), *args], env)
    output, sent = b"", False
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], 1)
        if not ready:
            continue
        try:
            chunk = os.read(fd, 1024)
        except OSError:
            break
        if not chunk:
            break
        output += chunk
        if not sent and b"Recovery key:" in output:
            os.write(fd, typed.encode() + b"\n")
            sent = True
    else:
        os.kill(pid, 9)
    _, status = os.waitpid(pid, 0)
    os.close(fd)
    return os.waitstatus_to_exitcode(status), output.decode(errors="replace")


def main():
    binary = Path(sys.argv[1]).resolve()
    if stored_key() is not None:
        fail("a hush key is already in this Secret Service; run this only in a throwaway D-Bus session")
    with tempfile.TemporaryDirectory(prefix="hush-secret-service-") as temp:
        env = {k: v for k, v in os.environ.items()
               if k not in {"HUSH_KEY", "HUSH_PROJECT", "HUSH_ENV", "CLAUDECODE"}}
        env["HUSH_HOME"] = str(Path(temp) / "vault")
        value = "secret-service-" + secrets.token_hex(16)
        try:
            run(binary, env, value)
        finally:
            subprocess.run(["secret-tool", "clear", *ITEM])
    print("PASS: setup stored the key, later commands reused it, and setup --recover restored it")


def run(binary, env, value):
    created = hush(binary, env, "setup")
    if created.returncode != 0 or "the OS keychain" not in created.stderr:
        fail("hush setup did not store the key in the Secret Service", created.stderr)
    key = stored_key()
    if not key:
        fail("hush setup succeeded but secret-tool finds no hush item")

    for args, data in ((("setup",), ""), (("project", "create", "demo"), ""),
                       (("-p", "demo", "set", "TOKEN"), value)):
        result = hush(binary, env, *args, data=data)
        if result.returncode != 0:
            fail("hush " + " ".join(args) + " failed with the stored key", result.stderr)
    got = hush(binary, env, "-p", "demo", "get", "TOKEN")
    if got.returncode != 0 or got.stdout != value:
        fail("a later hush process did not read the secret with the stored key", got.stderr)

    other = "-".join(secrets.token_hex(4) for _ in range(8))
    code, output = in_terminal(binary, env, ["setup", "--recover"], other)
    if code == 0 or "different hush key" not in output:
        fail("setup --recover accepted a different key over the stored one", output.replace(key, "<key>"))

    subprocess.run(["secret-tool", "clear", *ITEM], check=True)
    lost = hush(binary, env, "-p", "demo", "get", "TOKEN")
    if lost.returncode == 0 or "--recover" not in lost.stderr:
        fail("with the key removed from the Secret Service, get did not point to setup --recover", lost.stderr)

    code, output = in_terminal(binary, env, ["setup", "--recover"], key)
    if code != 0:
        fail("setup --recover did not restore the key", output.replace(key, "<key>"))
    if stored_key() != key:
        fail("setup --recover did not put the same key back in the Secret Service")
    got = hush(binary, env, "-p", "demo", "get", "TOKEN")
    if got.returncode != 0 or got.stdout != value:
        fail("the secret did not read back after setup --recover", got.stderr)


if __name__ == "__main__":
    main()
