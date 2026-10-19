"""Linux-only end-to-end vault checks through a real pseudo-terminal.

Drives the release binary with real Argon2id defaults. Uses generated test-only
passphrases; never real secrets. Usage: python3 tests/vault_pty.py target/release/maxonkey
"""

import base64
import fcntl
import os
import pty
import re
import select
import shutil
import statistics
import subprocess
import sys
import tempfile
import termios
import time

PASS_1 = "MK-CANARY-first passphrase " + os.urandom(4).hex()
PASS_2 = "MK-CANARY-second passphrase " + os.urandom(4).hex()
PASS_3 = "MK-CANARY-third passphrase " + os.urandom(4).hex()
UNLOCK_LIMIT_SECONDS = 2.0  # Spec 002 SC-002


class Session:
    """Runs maxonkey with a PTY as its controlling terminal; stdout/stderr are pipes."""

    def __init__(self, binary, args, umask=None, env=None):
        self.master, slave = pty.openpty()

        def child_setup():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
            if umask is not None:
                os.umask(umask)

        self.process = subprocess.Popen(
            [binary, *args],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            preexec_fn=child_setup,
            pass_fds=(slave,),
            env=env,
        )
        os.close(slave)
        self.screen = bytearray()

    def expect(self, text, timeout=15):
        deadline = time.monotonic() + timeout
        while text.encode() not in self.screen:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise AssertionError(f"timed out waiting for {text!r}; screen: {bytes(self.screen)!r}")
            ready, _, _ = select.select([self.master], [], [], min(remaining, 0.1))
            if ready:
                try:
                    self.screen.extend(os.read(self.master, 65536))
                except OSError:
                    pass
            elif self.process.poll() is not None:
                raise AssertionError(f"exited early ({self.process.returncode}) waiting for {text!r}: "
                                     f"{bytes(self.screen)!r} {self.process.stderr.read()!r}")
        return self.screen.decode(errors="replace")

    def send(self, line):
        os.write(self.master, line.encode() + b"\n")

    def finish(self, timeout=30):
        stdout, stderr = self.process.communicate(timeout=timeout)
        while select.select([self.master], [], [], 0.05)[0]:
            try:
                chunk = os.read(self.master, 65536)
            except OSError:
                break
            if not chunk:
                break
            self.screen.extend(chunk)
        os.close(self.master)
        return self.process.returncode, stdout.decode(), stderr.decode()


def run_plain(binary, args, **kwargs):
    return subprocess.run([binary, *args], capture_output=True, text=True, timeout=60, **kwargs)


def check_dumpable_cleared(session):
    """SR-026: /proc/<pid> of a non-dumpable process is owned by root."""
    path = f"/proc/{session.process.pid}/mem"
    owner = os.stat(path).st_uid
    if os.geteuid() != 0:
        assert owner == 0, f"{path} owned by uid {owner}; PR_SET_DUMPABLE not cleared"


def init_vault(binary, vault, umask=0):
    s = Session(binary, ["--vault", vault, "vault", "init"], umask=umask)
    s.expect("New master passphrase:")
    check_dumpable_cleared(s)
    s.send(PASS_1)
    s.expect("Repeat the passphrase:")
    s.send(PASS_1)
    screen = s.expect("Press Enter after you have stored the recovery key:")
    match = re.search(r"((?:[A-Z2-7]{4}-){12}[A-Z2-7]{4})", screen)
    assert match, f"recovery key not shown on the terminal: {screen!r}"
    recovery = match.group(1)
    s.send("")
    code, stdout, stderr = s.finish()
    assert code == 0, (code, stdout, stderr)
    assert "Vault created" in stdout
    assert recovery not in stdout + stderr, "recovery key leaked to stdout/stderr"
    for secret in (PASS_1,):
        assert secret not in stdout + stderr + bytes(s.screen).decode(errors="replace")
    return recovery


def unlock_attempt(binary, vault, passphrase):
    """Runs change-passphrase up to the unlock result; returns (exit code or None, seconds)."""
    s = Session(binary, ["--vault", vault, "vault", "change-passphrase"])
    s.expect("Current master passphrase:")
    start = time.monotonic()
    s.send(passphrase)
    deadline = time.monotonic() + 30
    while True:
        if b"New master passphrase:" in s.screen:
            elapsed = time.monotonic() - start
            s.process.kill()
            s.finish()
            return None, elapsed
        if s.process.poll() is not None:
            elapsed = time.monotonic() - start
            code, _, stderr = s.finish()
            return code, elapsed
        if time.monotonic() > deadline:
            raise AssertionError("unlock attempt timed out")
        ready, _, _ = select.select([s.master], [], [], 0.05)
        if ready:
            try:
                s.screen.extend(os.read(s.master, 65536))
            except OSError:
                pass


def change_passphrase(binary, vault, old, new):
    s = Session(binary, ["--vault", vault, "vault", "change-passphrase"])
    s.expect("Current master passphrase:")
    s.send(old)
    s.expect("New master passphrase:")
    s.send(new)
    s.expect("Repeat the passphrase:")
    s.send(new)
    code, stdout, stderr = s.finish()
    assert code == 0, (code, stdout, stderr)
    assert "Passphrase changed" in stdout


def recover(binary, vault, recovery, new):
    s = Session(binary, ["--vault", vault, "vault", "recover"])
    s.expect("Recovery key:")
    s.send(recovery.lower().replace("-", " "))
    s.expect("New master passphrase:")
    s.send(new)
    s.expect("Repeat the passphrase:")
    s.send(new)
    code, stdout, stderr = s.finish()
    assert code == 0, (code, stdout, stderr)
    assert "Passphrase reset" in stdout


def scan_for_canaries(directory, needles):
    for name in os.listdir(directory):
        data = open(os.path.join(directory, name), "rb").read()
        for needle in needles:
            raw = needle.encode()
            for form in (raw, needle.encode("utf-16-le"), raw.hex().encode(), base64.b64encode(raw)):
                assert form not in data, f"credential material found in {name}"


def main():
    if sys.platform != "linux":
        print("SKIP: Linux-only PTY vault checks")
        return
    binary = os.path.abspath(sys.argv[1])
    root = tempfile.mkdtemp(prefix="maxonkey-vault-test-")
    try:
        data_dir = os.path.join(root, "data", "maxonkey")
        vault = os.path.join(data_dir, "vault.sqlite3")

        # US1 / ST-13: create with umask 000; modes must still be owner-only.
        recovery = init_vault(binary, vault, umask=0)
        assert os.stat(data_dir).st_mode & 0o777 == 0o700, oct(os.stat(data_dir).st_mode)
        assert os.stat(vault).st_mode & 0o777 == 0o600, oct(os.stat(vault).st_mode)
        assert os.listdir(data_dir) == ["vault.sqlite3"], os.listdir(data_dir)
        print("PASS: init with recovery key; owner-only modes under umask 000")

        status = run_plain(binary, ["--vault", vault, "vault", "status"])
        assert status.returncode == 0, status
        for expected in ("Vault: INITIALIZED (LOCKED)", "Argon2id m=262144 KiB t=3 p=1",
                         "Recovery key: CONFIGURED", "Permissions: OWNER ONLY"):
            assert expected in status.stdout, status.stdout
        os.chmod(vault, 0o644)
        widened = run_plain(binary, ["--vault", vault, "vault", "status"])
        assert "chmod 600" in widened.stdout, widened.stdout
        os.chmod(vault, 0o600)
        print("PASS: vault status without passphrase; permission warning")

        # ST-10: second init refuses and leaves the file unchanged.
        before = open(vault, "rb").read()
        again = run_plain(binary, ["--vault", vault, "vault", "init"])
        assert again.returncode == 1 and "already exists" in again.stderr
        assert open(vault, "rb").read() == before
        print("PASS: init never overwrites")

        # ST-06 and SC-002: wrong passphrase gives exit 3; time the unlock.
        timings = []
        for _ in range(3):
            code, seconds = unlock_attempt(binary, vault, "MK-CANARY-wrong passphrase")
            assert code == 3, code
            timings.append(seconds)
        median = statistics.median(timings)
        print(f"INFO: unlock with default Argon2id took {median:.2f}s (median of 3)")
        assert median <= UNLOCK_LIMIT_SECONDS, f"unlock took {median:.2f}s"
        assert open(vault, "rb").read() == before
        print("PASS: wrong passphrase exits 3; file unchanged")

        # US3 / ST-17: change passphrase, old one stops working.
        change_passphrase(binary, vault, PASS_1, PASS_2)
        assert unlock_attempt(binary, vault, PASS_1)[0] == 3
        assert unlock_attempt(binary, vault, PASS_2)[0] is None
        print("PASS: change-passphrase")

        # US3: recover with the recovery key, then the new passphrase works.
        recover(binary, vault, recovery, PASS_3)
        assert unlock_attempt(binary, vault, PASS_2)[0] == 3
        assert unlock_attempt(binary, vault, PASS_3)[0] is None
        print("PASS: recover with recovery key")

        # ST-02 (002B part): no credential material in any file.
        scan_for_canaries(data_dir, [PASS_1, PASS_2, PASS_3, recovery, recovery.replace("-", "")])
        print("PASS: no passphrase or recovery key material on disk")

        # ST-11 / ST-12: no controlling terminal; stdin and environment are ignored.
        env = dict(os.environ, MAXONKEY_PASSPHRASE=PASS_3)
        for command in (["vault", "change-passphrase"], ["vault", "recover"]):
            result = subprocess.run(
                [binary, "--vault", vault, *command], input=f"{PASS_3}\n{PASS_1}\n{PASS_1}\n",
                capture_output=True, text=True, timeout=60, env=env, start_new_session=True,
            )
            assert result.returncode == 1, result
            assert "interactive terminal" in result.stderr, result.stderr
        fresh = os.path.join(root, "fresh", "vault.sqlite3")
        result = subprocess.run(
            [binary, "--vault", fresh, "vault", "init"], input=f"{PASS_1}\n{PASS_1}\n\n",
            capture_output=True, text=True, timeout=60, env=env, start_new_session=True,
        )
        assert result.returncode == 1 and "interactive terminal" in result.stderr, result
        assert not os.path.exists(fresh)
        assert unlock_attempt(binary, vault, PASS_3)[0] is None
        print("PASS: without a terminal, stdin and MAXONKEY_PASSPHRASE are never used")

        # ST-10: short or mismatched passphrases create nothing.
        other = os.path.join(root, "other", "vault.sqlite3")
        s = Session(binary, ["--vault", other, "vault", "init"])
        s.expect("New master passphrase:")
        s.send("too short")
        code, _, stderr = s.finish()
        assert code == 1 and "at least 12" in stderr, (code, stderr)
        s = Session(binary, ["--vault", other, "vault", "init"])
        s.expect("New master passphrase:")
        s.send(PASS_1)
        s.expect("Repeat the passphrase:")
        s.send(PASS_2)
        code, _, stderr = s.finish()
        assert code == 1 and "did not match" in stderr, (code, stderr)
        assert not os.path.exists(other)
        print("PASS: short and mismatched passphrases create no vault")

        # --no-recovery-key: recover reports that no key exists.
        bare = os.path.join(root, "bare", "vault.sqlite3")
        s = Session(binary, ["--vault", bare, "vault", "init", "--no-recovery-key"])
        s.expect("New master passphrase:")
        s.send(PASS_1)
        s.expect("Repeat the passphrase:")
        s.send(PASS_1)
        code, stdout, stderr = s.finish()
        assert code == 0 and "no recovery key" in stderr, (code, stdout, stderr)
        assert "Recovery key" not in bytes(s.screen).decode(errors="replace")
        result = run_plain(binary, ["--vault", bare, "vault", "recover"])
        assert result.returncode == 1 and "no recovery key" in result.stderr, result
        print("PASS: --no-recovery-key")
    finally:
        shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    main()
