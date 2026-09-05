"""Bound acceptance commands and their owned process groups on Linux/macOS.

This helper owns new POSIX sessions; it is not a cancellation API for remote
Titan systems. Runtime and cold-build limits are independent wall-clock bounds.
"""
import atexit
import sys
import math
import os
from pathlib import Path
import signal
import subprocess
import threading
import time
import json

DEFAULT_TIMEOUTS = {"runtime": 60.0, "build": 1200.0}
TERM_GRACE_SECONDS = 1.0
CLEANUP_TIMEOUT_SECONDS = 2.0
_ACTIVE = set()
_ACTIVE_LOCK = threading.RLock()


def _cleanup_active():
    with _ACTIVE_LOCK:
        active = tuple(_ACTIVE)
    # Snapshot before killing Python owners: their children may own independent
    # sessions whose watchdogs disappear when the owner receives SIGTERM.
    groups = _descendant_groups({process.pid for process in active}) if active else set()
    for group in groups:
        _signal_group(group, signal.SIGTERM)
    for group in groups:
        _signal_group(group, signal.SIGKILL)
    for process in active:
        terminate(process, grace_seconds=0)


def _descendant_groups(roots):
    listing = subprocess.run(["ps", "-axo", "pid=,ppid=,pgid="],
                             capture_output=True, text=True,
                             timeout=CLEANUP_TIMEOUT_SECONDS, check=True)
    rows = [tuple(map(int, line.split())) for line in listing.stdout.splitlines()
            if len(line.split()) == 3]
    owned = set(roots)
    while True:
        descendants = {pid for pid, parent, group in rows if parent in owned}
        if descendants <= owned:
            break
        owned.update(descendants)
    caller_group = os.getpgrp()
    return {group for pid, parent, group in rows
            if pid in owned and group != caller_group}


def _on_sigterm(signum, frame):
    _cleanup_active()
    raise SystemExit(128 + signum)


atexit.register(_cleanup_active)
if threading.current_thread() is threading.main_thread():
    signal.signal(signal.SIGTERM, _on_sigterm)


def timeout_seconds(phase="runtime"):
    if phase not in DEFAULT_TIMEOUTS:
        raise ValueError(f"unknown acceptance phase: {phase}")
    name = f"TITAN_{phase.upper()}_TIMEOUT_SECONDS"
    try:
        value = float(os.environ.get(name, DEFAULT_TIMEOUTS[phase]))
    except ValueError as error:
        raise ValueError(f"{name} must be a finite positive number") from error
    if not math.isfinite(value) or value <= 0:
        raise ValueError(f"{name} must be a finite positive number")
    return value


class TimeoutExpired(subprocess.TimeoutExpired):
    def __init__(self, cmd, timeout, phase, output=None, stderr=None):
        super().__init__(cmd, timeout, output=output, stderr=stderr)
        self.phase = phase

    def __str__(self):
        # Commands may include credentials: identify the phase without argv.
        return f"acceptance {self.phase} phase exceeded {self.timeout:g}s wall-clock limit"


def _signal_group(pid, sig):
    try:
        os.killpg(pid, sig)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        # Darwin returns EPERM for a group containing only zombies. Never
        # suppress a permission error when a live member still needs cleanup.
        if _group_has_live_members(pid):
            raise
        return False


def _group_has_live_members(pgid):
    listing = subprocess.run(["ps", "-axo", "pgid=,stat="], capture_output=True,
                             text=True, timeout=CLEANUP_TIMEOUT_SECONDS, check=True)
    return any(parts[0] == str(pgid) and not parts[1].startswith("Z")
               for line in listing.stdout.splitlines() if len(parts := line.split()) == 2)


class Popen:
    """Popen-compatible acceptance process with a lifetime watchdog.

    stdout/stderr and other subprocess options are passed through. Each process
    owns its session, including descendants retaining captured output pipes.
    Optional project/instance identify registrations eligible for cleanup; PID
    ownership is verified too, so another host's registration is preserved.
    """
    def __init__(self, args, *, phase="runtime", timeout=None, project=None,
                 instance=None, **kwargs):
        if os.name != "posix":
            raise RuntimeError("acceptance process cleanup supports Linux/macOS POSIX sessions only")
        self.phase = phase
        configured = timeout_seconds(phase)
        self.timeout = configured if timeout is None else float(timeout)
        if not math.isfinite(self.timeout) or self.timeout <= 0:
            raise ValueError("acceptance timeout must be finite and positive")
        child_env = dict(os.environ if kwargs.get("env") is None else kwargs["env"])
        inherited_deadline = child_env.get("TITAN_ACCEPTANCE_DEADLINE_EPOCH")
        if inherited_deadline is not None:
            try:
                deadline = float(inherited_deadline)
                if not math.isfinite(deadline) or deadline <= 0:
                    raise ValueError()
            except ValueError as error:
                raise ValueError("TITAN_ACCEPTANCE_DEADLINE_EPOCH must be finite and positive") from error
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutExpired(args, 0, phase)
            self.timeout = min(self.timeout, remaining)
        if kwargs.get("start_new_session") is False or kwargs.get("process_group") is not None:
            raise ValueError("acceptance processes must own a new session")
        kwargs["start_new_session"] = True
        self._project = Path(project).resolve() if project is not None else None
        self._instance = instance
        self._lock = threading.RLock()
        self._cleaned = False
        self._expired = False
        # Nested helpers own independent sessions. Give them time to finish
        # cleanup before this process group reaches its own lifetime deadline.
        headroom = min(5.0, self.timeout / 2)
        child_env["TITAN_ACCEPTANCE_DEADLINE_EPOCH"] = str(time.time() + self.timeout - headroom)
        kwargs["env"] = child_env
        self._process = subprocess.Popen(args, **kwargs)
        self._deadline = time.monotonic() + self.timeout
        self._timer = threading.Timer(self.timeout, self._expire)
        self._timer.daemon = True
        with _ACTIVE_LOCK:
            _ACTIVE.add(self)
        self._timer.start()

    def __getattr__(self, name):
        return getattr(self._process, name)

    def _expire(self):
        with self._lock:
            if self._cleaned:
                return
            self._expired = True
        print(str(self._error()), file=sys.stderr, flush=True)
        terminate(self)

    def _remaining(self, timeout):
        remaining = max(0, self._deadline - time.monotonic())
        return remaining if timeout is None else min(remaining, timeout)

    def _error(self, error=None):
        limit = self.timeout if error is None else min(self.timeout, error.timeout)
        return TimeoutExpired(self.args, limit, self.phase,
                              getattr(error, "output", None), getattr(error, "stderr", None))

    def poll(self):
        if self._expired:
            terminate(self)
            raise self._error()
        result = self._process.poll()
        if result is not None:
            terminate(self)
        return result

    def wait(self, timeout=None):
        try:
            result = self._process.wait(timeout=self._remaining(timeout))
        except subprocess.TimeoutExpired as error:
            terminate(self)
            raise self._error(error) from error
        terminate(self)
        if self._expired:
            raise self._error()
        return result

    def communicate(self, input=None, timeout=None):
        try:
            result = self._process.communicate(input=input, timeout=self._remaining(timeout))
        except subprocess.TimeoutExpired as error:
            terminate(self)
            # Never do an unbounded second communicate: an escaped descendant
            # may still hold the pipes. Preserve the captured partial evidence.
            try:
                output, stderr = self._process.communicate(timeout=CLEANUP_TIMEOUT_SECONDS)
                error.output, error.stderr = output, stderr
            except subprocess.TimeoutExpired:
                pass
            raise self._error(error) from error
        terminate(self)
        if self._expired:
            raise TimeoutExpired(self.args, self.timeout, self.phase, *result)
        return result

    def terminate(self):
        terminate(self)

    def kill(self):
        terminate(self)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        terminate(self)
        for stream in (self.stdout, self.stderr, self.stdin):
            if stream is not None:
                stream.close()

    def _registrations(self):
        if self._project is None or self._instance is None:
            return []
        owned = []
        for path in (self._project / "target/titan/instances").glob("*.json"):
            try:
                data = json.loads(path.read_text())
                if data.get("instance_id") != self._instance or Path(data["project"]).resolve() != self._project:
                    continue
                pid = data["pid"]
                if type(pid) is not int:
                    continue
                if pid != self.pid and os.getpgid(pid) != self.pid:
                    continue
                owned.append((path, data))
            except (OSError, ValueError, KeyError, TypeError):
                continue
        return owned


def terminate(process, *, grace_seconds=TERM_GRACE_SECONDS):
    """Idempotently reap an owned group, even when its leader already exited.

    Raw Popen is supported only when its caller used start_new_session=True.
    Such callers own registration cleanup themselves.
    """
    if not isinstance(process, Popen):
        if _signal_group(process.pid, signal.SIGTERM):
            time.sleep(grace_seconds)
            process.poll()  # Reap a zombie leader before macOS killpg.
            _signal_group(process.pid, signal.SIGKILL)
        process.wait(timeout=CLEANUP_TIMEOUT_SECONDS)
        return
    with process._lock:
        if process._cleaned:
            return
        process._timer.cancel()
        registrations = process._registrations()
        try:
            if _signal_group(process.pid, signal.SIGTERM):
                # A reaped leader does not imply its descendants have exited.
                time.sleep(grace_seconds)
                process._process.poll()  # Reap before macOS killpg on zombie-only groups.
                _signal_group(process.pid, signal.SIGKILL)
            process._process.wait(timeout=CLEANUP_TIMEOUT_SECONDS)
        finally:
            for path, original in registrations:
                try:
                    if json.loads(path.read_text()) == original:
                        path.unlink()
                except (OSError, ValueError):
                    pass
            process._cleaned = True
            with _ACTIVE_LOCK:
                _ACTIVE.discard(process)


def graceful_shutdown(process):
    """Ask the host to exit, preserving evidence of its registration cleanup.

    Call terminate in a finally block after checking the host removed its own
    registration. A timed-out shutdown still cleans the owned process group.
    """
    process._process.terminate()
    try:
        result = process._process.wait(timeout=process._remaining(None))
    except subprocess.TimeoutExpired as error:
        terminate(process)
        raise process._error(error) from error
    if process._expired:
        terminate(process)
        raise process._error()
    return result


def run(args, *, input=None, capture_output=False, check=False, timeout=None,
        phase="runtime", **kwargs):
    if input is not None:
        if "stdin" in kwargs:
            raise ValueError("stdin and input arguments may not both be used")
        kwargs["stdin"] = subprocess.PIPE
    if capture_output:
        if "stdout" in kwargs or "stderr" in kwargs:
            raise ValueError("stdout and stderr arguments may not be used with capture_output")
        kwargs.update(stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    with Popen(args, phase=phase, timeout=timeout, **kwargs) as process:
        stdout, stderr = process.communicate(input=input)
        result = subprocess.CompletedProcess(args, process.returncode, stdout, stderr)
        if check:
            result.check_returncode()
        return result


def check_output(args, *, timeout=None, phase="runtime", **kwargs):
    if "stdout" in kwargs:
        raise ValueError("stdout argument not allowed, it will be overridden")
    return run(args, stdout=subprocess.PIPE, timeout=timeout, phase=phase,
               check=True, **kwargs).stdout
