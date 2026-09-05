#!/usr/bin/env python3
"""Inject failures in the real native harnesses and verify retained evidence/cleanup."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[1]


def main():
    for name, script in [
        ('rpg-control', 'scripts/test-control-loop.py'),
        ('arena-control', 'games/arena/scripts/test-control.py'),
    ]:
        with tempfile.TemporaryDirectory(prefix='titan-evidence-verification-') as directory:
            root = Path(directory)
            result = subprocess.run(
                [sys.executable, str(REPO / script)], cwd=REPO,
                env=dict(os.environ, TITAN_ACCEPTANCE_EVIDENCE_DIR=str(root),
                         TITAN_ACCEPTANCE_FAIL=f'{name}:diagnostic'),
                capture_output=True, text=True, timeout=600,
            )
            assert result.returncode == 1, (name, result.returncode, result.stderr[-4000:])
            packages = list(root.iterdir())
            assert len(packages) == 1, (name, result.stderr[-4000:])
            package = packages[0]
            files = {path.name for path in package.iterdir()}
            expected = {'context.json', 'commands.log', 'runtime.log', 'bundle.json',
                        'api.txt', 'capture.png', 'latest-capture.ppm'}
            assert files == expected, (name, files)
            context = json.loads((package / 'context.json').read_text())
            assert 'diagnostic' in json.dumps(context), context
            assert context['process_ids'], context
            for pid in context['process_ids']:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    pass
                else:
                    raise AssertionError(f'{name}: owned runtime {pid} survived failure')
            for registration in (REPO / 'games/arena/target/titan/instances').glob('*.json'):
                assert json.loads(registration.read_text())['pid'] not in context['process_ids']
            assert sum(path.stat().st_size for path in package.iterdir()) <= 6 * 1024 * 1024
            print(f'{name}: original failure status, bounded package, runtime exit and registration cleanup verified')


if __name__ == '__main__':
    main()
