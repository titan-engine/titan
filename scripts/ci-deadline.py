#!/usr/bin/env python3
"""Run an Actions shell step inside the job's shared acceptance deadline."""
import os
from pathlib import Path
import sys
import time

import acceptance_process as processes


def remaining(started, now, budget=45 * 60):
    return max(0, budget - (now - started))


def main():
    # The first run step persists the deadline for later steps via GITHUB_ENV.
    now = time.time()
    started = float(os.environ.get('TITAN_CI_STARTED_AT', now))
    if 'TITAN_CI_STARTED_AT' not in os.environ:
        with Path(os.environ['GITHUB_ENV']).open('a') as output:
            output.write(f'TITAN_CI_STARTED_AT={started}\n')
    seconds = remaining(started, now)
    if seconds <= 0:
        raise SystemExit('CI acceptance deadline exhausted; preserving evidence-collection headroom')
    try:
        result = processes.run(['bash', '--noprofile', '--norc', '-eo', 'pipefail', sys.argv[1]],
                               phase='build', timeout=seconds)
    except processes.TimeoutExpired:
        raise SystemExit('CI acceptance deadline exceeded; owned step processes stopped') from None
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
