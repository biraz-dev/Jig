"""Linux PTY smoke check: real terminal startup, timer, input, resize, cleanup."""
import fcntl
import os
import pathlib
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time
from datetime import datetime, timedelta, timezone

binary = str(pathlib.Path("target/debug/jig").resolve())
with tempfile.TemporaryDirectory(prefix="jig-pty-") as directory:
    state = str(pathlib.Path(directory) / "jig.json")
    at = (datetime.now(timezone.utc) + timedelta(days=2)).isoformat(timespec="seconds")
    subprocess.run([binary, "--state", state, "deadline", "add", "Sample opening", at], check=True)
    master, slave = pty.openpty()
    original = termios.tcgetattr(slave)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    process = subprocess.Popen([binary, "--state", state], stdin=slave, stdout=slave, stderr=slave, env={**os.environ, "TERM": "xterm-256color"})

    def read_for(seconds):
        chunks = []
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if select.select([master], [], [], min(0.1, max(0, until - time.monotonic())))[0]:
                chunks.append(os.read(master, 65536))
        return b"".join(chunks)

    try:
        first = read_for(0.5)
        assert b"CALCULATOR" in first and b"MAJOR DEADLINE" in first and b"Sample opening" in first, first
        tick = read_for(1.5)
        assert tick, "Countdown did not redraw while idle"
        os.write(master, b"2aSample PTY task\r")
        read_for(0.3)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 14, 60, 0, 0))
        import signal
        process.send_signal(signal.SIGWINCH)
        read_for(0.3)
        os.write(master, b"q")
        process.wait(timeout=5)
        assert process.returncode == 0
        assert termios.tcgetattr(slave) == original, "Terminal settings were not restored"
        tasks = subprocess.check_output([binary, "--state", state, "tasks"], text=True)
        assert "Sample PTY task" in tasks
        print("PTY: startup, countdown tick, keyboard persistence, resize and terminal restoration passed")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)
