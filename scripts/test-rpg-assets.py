#!/usr/bin/env python3
"""Bounded file-backed RPG checks; --gpu additionally checks a relocated app bundle."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib

ROOT = Path(__file__).resolve().parents[1]
REFERENCE = 'f7a298f62ad75c1c'


def png(color, width=8, height=10):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    pixels = (b'\0' + bytes(color) * width) * height
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b''))


def run(command, cwd, success=True, timeout=15):
    result = subprocess.run(list(map(str, command)), cwd=cwd, capture_output=True, text=True, timeout=timeout)
    assert (result.returncode == 0) == success, (command, result.stdout, result.stderr)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gpu', action='store_true')
    options = parser.parse_args()
    build = ['cargo', 'build', '-p', 'titan-cli', '-p', 'titan', '--bin', 'titan',
             '--example', 'procedural_rpg', '--example', 'replay_rpg']
    run(build, ROOT, timeout=180)
    metadata = json.loads(run(['cargo', 'metadata', '--no-deps', '--format-version', '1'], ROOT).stdout)
    target = Path(metadata['target_directory'])
    executable = target / 'debug/examples/procedural_rpg'
    verifier = target / 'debug/examples/replay_rpg'
    cli = target / 'debug/titan'
    with tempfile.TemporaryDirectory(prefix='titan asset acceptance ') as directory:
        project = Path(directory)
        assets = project / 'art outside checkout'
        assets.mkdir()
        sprite = assets / 'player.png'
        sprite.write_bytes((ROOT / 'assets/player.png').read_bytes())

        def reference(*arguments):
            output = run([executable, *arguments], project).stdout
            match = re.search(r'\(3 shards, shrine active: true, checksum: ([0-9a-f]{16})\)', output)
            assert match, output
            return match[1]

        assert reference('--generated-assets') == REFERENCE
        assert reference('--assets-dir', assets) == REFERENCE
        # Reuse the same executable after replacing an external asset; no Cargo call occurs here.
        binary_before = (executable.stat().st_mtime_ns, executable.stat().st_size)
        sprite.write_bytes(png((245, 40, 110, 255)))
        first = reference('--assets-dir', assets)
        sprite.write_bytes(png((30, 220, 230, 255)))
        second = reference('--assets-dir', assets)
        assert len({REFERENCE, first, second}) == 3
        assert binary_before == (executable.stat().st_mtime_ns, executable.stat().st_size)

        instance = f'assets-{os.getpid()}'
        def call(*arguments):
            return json.loads(run([cli, '--format', 'json', '--project', project,
                                   '--instance', instance, *arguments], project).stdout)

        # Record with the substituted sprite, then verify in a fresh process with the same asset.
        with tempfile.TemporaryFile(mode='w+') as log:
            process = subprocess.Popen([str(executable), '--serve', '--project', str(project),
                '--instance', instance, '--assets-dir', str(assets), '--run-for-ms', '20000'],
                cwd=project, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 5
                while not call('instances')['instances']:
                    assert process.poll() is None and time.monotonic() < deadline, 'asset session did not register'
                    time.sleep(.02)
                actions = ['right'] * 2 + ['down'] * 3 + ['right'] * 6
                for frame, action in enumerate(actions, 1):
                    call('input', str(frame), '--actions', json.dumps({action: {'kind': 'button', 'value': True}}))
                call('step', '11')
                save = call('query', 'save')['response']['value']
                recording = call('query', 'recording')['response']['value']
                assert recording['final_checksum'] == second
                record_path = project / 'custom-sprite-recording.json'
                record_path.write_text(json.dumps(recording))
                verified = json.loads(run([verifier, record_path, '--assets-dir', assets], project).stdout)
                assert verified['save'] == save and verified['checksum'] == second
                rejected = run([verifier, record_path, '--assets-dir', ROOT / 'assets'], project, success=False)
                assert 'pixels mismatch' in rejected.stderr.lower() and 'same player image' in rejected.stderr.lower(), rejected.stderr
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=10)
                log.seek(0)
                assert process.returncode == 0, log.read()
        assert not call('instances')['instances']

        for label, data in [('missing', None), ('corrupt', b'not a PNG'),
                            ('oversized', b'x' * (256 * 1024 + 1)),
                            ('dimensions', png((0, 0, 0, 255), 65, 1))]:
            if data is None:
                sprite.unlink()
            else:
                sprite.write_bytes(data)
            failed = run([executable, '--serve', '--project', project, '--instance', instance,
                          '--run-for-ms', '1000', '--assets-dir', assets], project, success=False)
            assert str(sprite) in failed.stderr and '--assets-dir' in failed.stderr, (label, failed.stderr)
            assert not call('instances')['instances'], label

        if options.gpu:
            result = run([sys.executable, ROOT / 'scripts/build-rpg-app.py'], ROOT, timeout=180)
            bundle = Path(result.stdout.strip().splitlines()[-1])
            relocated = project / 'Relocated RPG.app'
            shutil.copytree(bundle, relocated)
            bundled_player = relocated / 'Contents/MacOS/play_rpg'
            bundled_sprite = relocated / 'Contents/Resources/assets/player.png'
            assert bundled_sprite.read_bytes() == (ROOT / 'assets/player.png').read_bytes()
            for data in [(ROOT / 'assets/player.png').read_bytes(), png((100, 90, 250, 255))]:
                bundled_sprite.write_bytes(data)
                played = run([bundled_player, '--replay', '--frames', '2', '--run-for-ms', '5000'], project)
                # Exit is requested at the threshold; queued native redraws may
                # still present before the event loop finishes shutting down.
                rendered = re.search(r'rendered (\d+) GPU frames;', played.stdout)
                assert rendered and int(rendered[1]) >= 2, played.stdout
                state = json.loads(played.stdout.split('; ', 1)[1])
                assert state['collected_shards'] == 3 and state['shrine_active'] is True, state
            bundled_sprite.unlink()
            failed = run([bundled_player, '--replay', '--frames', '2'], ROOT, success=False)
            assert str(bundled_sprite) in failed.stderr, failed.stderr  # No fallback to cwd/assets.
            # An explicit external directory overrides even a missing bundled resource.
            sprite.write_bytes(png((100, 90, 250, 255)))
            run([bundled_player, '--assets-dir', assets, '--replay', '--frames', '2', '--run-for-ms', '5000'], project)
    print('RPG asset acceptance passed: exact generated/file reference, external replacement without rebuild, custom recording replay, bounded startup failures'
          + (', relocated native GPU bundle and explicit override' if options.gpu else ''))


if __name__ == '__main__':
    main()
