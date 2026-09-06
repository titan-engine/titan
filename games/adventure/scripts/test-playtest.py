#!/usr/bin/env python3
"""Independent #86 assembled-session checks through the public native host/CLI.

Reference routes supply navigation only. Extra input, semantic assertions and
mistake/recovery scenarios are independently authored. This is not a human
usability study, browser test, or below-floor fixture.
"""
import argparse
import hashlib
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys
import tempfile
import time

GAME = Path(__file__).resolve().parents[1]
REPO = GAME.parents[1]
sys.path.insert(0, str(REPO / 'scripts'))
import acceptance_process as processes
from acceptance_evidence import FailureEvidence

SEMANTIC = ('phase', 'room', 'characters', 'active_character', 'block', 'puzzle',
            'session_tick', 'consumed_input', 'blocked_actions', 'recovery_message_ticks')


def main(evidence, log, output):
    def run(args, **kwargs):
        evidence.record_command(args, None)
        result = processes.run(args, **kwargs)
        evidence.record_command(args, result)
        result.check_returncode()
        return result

    def target(root, extra):
        run(['cargo', 'build', '--manifest-path', str(root / 'Cargo.toml'), *extra],
            phase='build', stdout=log, stderr=log)
        metadata = run(['cargo', 'metadata', '--no-deps', '--format-version', '1',
                        '--manifest-path', str(root / 'Cargo.toml')],
                       capture_output=True, text=True, phase='build')
        return Path(json.loads(metadata.stdout)['target_directory']) / 'debug'

    binary = target(GAME, ['--bin', 'titan-adventure']) / 'titan-adventure'
    cli = target(REPO, ['-p', 'titan-cli']) / 'titan'
    report = {'source_revision': run(['git', 'rev-parse', 'HEAD'], capture_output=True,
                                     text=True).stdout.strip(),
              'runner_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'recorded_at_utc': datetime.now(timezone.utc).isoformat(),
              'scenarios': {}}

    class Session:
        def __init__(self, name):
            self.name = name
            self.instance = f'playtest-86-{os.getpid()}-{name}'
            self.steps = []
            self.checkpoints = []
            self.replays = 0

        def call(self, *args):
            result = run([str(cli), '--format', 'json', '--project', str(GAME),
                          '--instance', self.instance, *map(str, args)],
                         capture_output=True, text=True)
            data = json.loads(result.stdout)
            evidence.observe(data)
            assert data['status'] == 'success', data
            return data

        def state(self):
            return self.call('query', 'state')['response']['value']

        def invoke(self, name, arguments=None):
            self.steps.append({'invoke': name, 'arguments': arguments or {}})
            return self.call('invoke', name, '--arguments', json.dumps(arguments or {}))

        def drive(self, actions=(), ticks=1):
            self.steps.append({'actions': list(actions), 'ticks': ticks})
            frame = self.call('status')['response']['current_frame']
            encoded = json.dumps({a: {'kind': 'button', 'value': True} for a in actions})
            for offset in range(1, ticks + 1):
                self.call('input', frame + offset, '--actions', encoded)
            self.call('step', ticks)
            return self.state()

        def mark(self, label, replay=True):
            before = self.state()
            item = {'label': label, 'state': before}
            if replay:
                recording = self.call('query', 'recording')['response']['value']
                encoded = json.dumps(recording, sort_keys=True).encode()
                item['recording_sha256'] = hashlib.sha256(encoded).hexdigest()
                with tempfile.TemporaryDirectory(prefix='adventure-playtest-') as temporary:
                    path = Path(temporary) / 'replay.json'
                    path.write_text(json.dumps({'recording': recording}))
                    self.call('invoke', 'replay', '--arguments-file', path)
                after = self.state()
                for key in SEMANTIC:
                    assert after[key] == before[key], (self.name, label, key, before, after)
                self.replays += 1
            self.checkpoints.append(item)
            return before

        def __enter__(self):
            self.process = processes.Popen(
                [str(binary), '--serve', '--instance', self.instance,
                 '--run-for-ms', '300000'], timeout=300,
                project=GAME, instance=self.instance, cwd=GAME, stdout=log, stderr=log)
            evidence.record_process(self.process)
            deadline = time.monotonic() + 10
            try:
                while not any(i['instance_id'] == self.instance
                              for i in self.call('instances')['instances']):
                    assert self.process.poll() is None, 'host exited before discovery'
                    assert time.monotonic() < deadline, 'discovery exceeded 10 seconds'
                    time.sleep(.05)
            except BaseException:
                processes.terminate(self.process)
                raise
            return self

        def __exit__(self, *_):
            try:
                processes.graceful_shutdown(self.process)
            finally:
                processes.terminate(self.process)
            assert not any(i['instance_id'] == self.instance
                           for i in self.call('instances')['instances'])
            report['scenarios'][self.name] = {'operations': self.steps,
                'checkpoints': self.checkpoints, 'semantic_replays': self.replays,
                'owned_host_cleaned_up': True}

    def route(s, filename):
        for segment in json.loads((GAME / 'tests' / filename).read_text()):
            s.drive(segment['actions'], segment['ticks'])
            label = segment.get('checkpoint')
            if not label:
                continue
            state = s.state()
            p = state['puzzle']
            if label == 'block-support':
                assert state['characters']['jumper']['support'] == 'heavy-block', state
            elif label == 'plate-a':
                assert p['plates'][0]['occupants'] == ['jumper'] and p['door']['open'], state
            elif label == 'plate-b':
                assert p['plates'][1]['occupants'] == ['strong'] and p['plates'][0]['pressed'], state
            elif label == 'exchange':
                assert not p['plates'][0]['pressed'] and p['plates'][1]['pressed'], state
            elif label == 'jumper-exit':
                assert p['exit']['jumper'] and not p['complete'], state
            elif label == 'complete':
                assert p['complete'] and all(p['exit'].values()), state
            # Wait at support/plate checkpoints to vary timing while preserving
            # geometry. No airborne waits are inserted into jump segments.
            if label != 'complete':
                settled = s.drive((), 7)
                assert settled['characters']['jumper']['grounded'], settled
            if label == 'exchange':
                # A is now empty. Deliberately abandon B, observe the closed
                # door, then recover by returning Strong to the same plate.
                s.drive(['switch'])
                s.drive()
                abandoned = s.drive(['down'], 8)
                assert not abandoned['puzzle']['door']['open'], abandoned
                recovered = s.drive(['up'], 8)
                assert recovered['puzzle']['plates'][1]['pressed'], recovered
                assert recovered['puzzle']['door']['open'], recovered
                s.drive(['switch'])
                s.drive()
            s.mark(f'room-{state["room"]}-{label}')

    held = ['confirm', 'right', 'jump', 'switch', 'interact']
    for name, filename in [('two-push', 'block-solution.json'),
                           ('one-push', 'block-intermediate-solution.json')]:
        with Session(name) as s:
            start = s.state()
            assert start['phase'] == 'start'
            transitioned = s.drive(held)
            assert transitioned['phase'] == 'playing'
            continued = s.drive(held, 5)
            assert continued['characters'] == start['characters']
            assert continued['active_character'] == 'jumper'
            s.drive()
            # An independently chosen safe excursion, then return to route.
            s.drive(['left', 'down'], 5)
            s.drive(['right', 'up'], 5)
            s.drive()
            route(s, 'puzzle-solution.json')
            completed = s.state()
            assert completed['phase'] == 'room_complete'
            frozen = s.drive(['left', 'jump', 'switch', 'interact'], 9)
            assert frozen['characters'] == completed['characters']
            assert frozen['session_tick'] == completed['session_tick']
            s.drive()
            next_room = s.drive(held)
            assert next_room['room'] == 2 and next_room['phase'] == 'playing'
            next_held = s.drive(held, 5)
            assert next_held['characters'] == next_room['characters']
            assert next_held['block'] == next_room['block']
            s.drive()
            s.mark('continued-held-gates')
            s.drive(['left'], 4)
            s.drive(['right'], 4)
            s.drive()
            route(s, filename)
            final = s.state()
            assert final['phase'] == 'slice_complete'
            assert final['block']['moves'] == (2 if name == 'two-push' else 1), final
            s.drive()
            again = s.drive(held)
            assert again['phase'] == 'playing' and again['room'] == 1
            still = s.drive(held, 4)
            assert still['characters'] == again['characters']
            s.mark('play-again-held-gates')

    with Session('mistakes') as s:
        s.invoke('select_room', {'room': 2})
        initial = s.state()
        wrong = s.drive(['interact', 'up'])
        assert wrong['block']['last_rejection'] == 'wrong_character', wrong
        assert wrong['block']['moves'] == 0
        s.drive()
        # Switch mid-jump while jump, movement and interact remain held.
        airborne = s.drive(['jump', 'right'])
        assert airborne['characters']['jumper']['y'] > 0
        switched = s.drive(['jump', 'right', 'interact', 'switch'])
        held_switch = s.drive(['jump', 'right', 'interact', 'switch'], 5)
        assert switched['active_character'] == held_switch['active_character'] == 'strong'
        assert held_switch['characters']['strong']['x'] == initial['characters']['strong']['x']
        assert held_switch['characters']['strong']['y'] == 0
        assert held_switch['characters']['jumper']['x'] == airborne['characters']['jumper']['x']
        assert held_switch['characters']['jumper']['y'] != airborne['characters']['jumper']['y']
        # Fresh unrelated direction works without releasing held right.
        fresh = s.drive(['right', 'down'])
        assert fresh['characters']['strong']['z'] == initial['characters']['strong']['z'] + 60
        assert fresh['characters']['strong']['x'] == initial['characters']['strong']['x']
        s.drive((), 40)
        s.mark('midair-held-switch-and-safe-landing')
        s.invoke('restart')
        s.drive(['switch'])
        s.drive()
        s.drive(['right'], 33)
        s.drive(['interact', 'up'])
        s.drive()
        pushed = s.mark('first-push-before-failed-jump')
        # Strong tries the block from the documented south stance. Jump is
        # too low; after landing the valid push arrangement must persist.
        s.drive(['jump', 'up'], 30)
        failed = s.drive((), 20)
        assert failed['characters']['strong']['support'] == 'floor', failed
        assert failed['characters']['strong']['y'] == 0
        assert failed['block'] == pushed['block']
        s.mark('strong-cannot-mount-block-safe-retry')
        # Walk around east side to reverse the intermediate socket.
        current = s.state()['characters']['strong']
        s.drive(['right'], 17)
        s.drive(['up'], round((current['z'] - 3500) / 60))
        s.drive(['left'], 17)
        reversed_state = s.drive(['interact', 'down'])
        assert reversed_state['block']['moves'] == 2, reversed_state
        assert reversed_state['block']['socket'] == initial['block']['socket'], reversed_state
        s.mark('reverse-intermediate-after-failed-jump')
        s.drive()
        s.drive(['jump'])
        before_reset = s.state()
        frame = s.call('status')['response']['current_frame']
        s.call('input', frame + 1, '--actions', '{"right":{"kind":"button","value":true}}')
        s.invoke('restart')
        reset = s.state()
        assert reset['frame'] == before_reset['frame']
        assert reset['session_generation'] == before_reset['session_generation'] + 1
        assert reset['characters'] == initial['characters'] and reset['block'] == initial['block']
        assert reset['pending_inputs'] == 0 and reset['recovery_message_ticks'] == 0
        assert s.drive()['characters'] == initial['characters']
        s.mark('restart-airborne-discards-pending-input')

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + '\n')
    print(f'Independent adventure playtest: {len(report["scenarios"])} scenarios, '
          f'{sum(v["semantic_replays"] for v in report["scenarios"].values())} semantic replays passed; {output}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path,
                        default=GAME / 'target/playtest-86/semantic.json')
    args = parser.parse_args()
    with FailureEvidence('adventure-playtest', repo=REPO) as evidence:
        with evidence.runtime_log() as log:
            main(evidence, log, args.output)
