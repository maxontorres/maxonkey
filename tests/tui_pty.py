"""Linux-only end-to-end terminal lifecycle checks using the standard library."""

import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import time


def read_until(master, expected, process):
    output = bytearray()
    deadline = time.monotonic() + 8
    while expected not in output:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise AssertionError(f"Timed out waiting for {expected!r}; received {output!r}")
        readable, _, _ = select.select([master], [], [], min(remaining, 0.1))
        if readable:
            output.extend(os.read(master, 65536))
        elif process.poll() is not None:
            raise AssertionError(f"TUI exited early: {process.returncode}; {output!r}")
    return bytes(output)


def set_size(slave, width, height):
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))


def check_exit(binary, exit_key, navigate=False):
    master, slave = pty.openpty()
    process = None
    try:
        set_size(slave, 80, 24)
        original = termios.tcgetattr(slave)

        def establish_terminal():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        process = subprocess.Popen(
            [binary], stdin=slave, stdout=slave, stderr=slave,
            preexec_fn=establish_terminal,
        )
        output = read_until(master, b"q/Esc/Ctrl+C Exit", process)
        assert b"\x1b[?1049h" in output, "alternate screen was not entered"
        assert b"Dashboard" in output
        if navigate:
            os.write(master, b"2")
            read_until(master, b"Shift+Tab", process)
            os.write(master, b"3")
            read_until(master, b"Software", process)
            set_size(slave, 40, 10)
            os.kill(process.pid, signal.SIGWINCH)
            read_until(master, b"Resize to 64x22", process)
            set_size(slave, 80, 24)
            os.kill(process.pid, signal.SIGWINCH)
            read_until(master, b"q/Esc/Ctrl+C Exit", process)
        os.write(master, exit_key)
        cleanup = read_until(master, b"\x1b[?1049l", process)
        assert process.wait(timeout=5) == 0, "normal exit returned failure"
        # Cursor and screen restoration may arrive in separate writes.
        while select.select([master], [], [], 0.1)[0]:
            cleanup += os.read(master, 65536)
        assert termios.tcgetattr(slave) == original, "terminal modes were not restored"
        assert b"\x1b[?25h" in cleanup, "cursor was not restored"
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        os.close(master)
        os.close(slave)


def main():
    if sys.platform != "linux":
        raise SystemExit("This PTY check requires Linux; use cargo test on other platforms.")
    binary = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/release/maxonkey")
    for exit_key in [b"q", b"\x1b", b"\x03"]:
        check_exit(binary, exit_key, navigate=exit_key == b"q")
        print(f"PASS: TUI exit {exit_key!r}, alternate screen, cursor, and terminal modes restored")


if __name__ == "__main__":
    main()
