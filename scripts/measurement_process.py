"""Fresh-child RSS and wall-time measurement shared by headless workloads."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

import acceptance_process as processes

REPO = Path(__file__).resolve().parents[1]


def measure(command, timeout):
    # A file avoids pipe deadlock. wait4 reports this child's high-water RSS,
    # unlike RUSAGE_CHILDREN, which accumulates unrelated builds and earlier runs.
    with tempfile.TemporaryFile(mode='w+') as output:
        started = time.monotonic()
        process = subprocess.Popen(command, cwd=REPO, stdout=output, start_new_session=True)
        try:
            while True:
                pid, status, usage = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    break
                if time.monotonic() - started >= timeout:
                    raise TimeoutError(f'workload exceeded {timeout}s: {command}')
                time.sleep(0.01)
        finally:
            processes.terminate(process)
        elapsed = time.monotonic() - started
        if process.returncode:
            raise RuntimeError(f'workload exited {process.returncode}: {command}')
        output.seek(0)
        workload = json.load(output)
    return {
        'process_wall_seconds': elapsed,
        'peak_process_rss_bytes': usage.ru_maxrss * (1 if sys.platform == 'darwin' else 1024),
        'workload': workload,
    }

