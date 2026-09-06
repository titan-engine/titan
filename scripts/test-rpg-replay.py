#!/usr/bin/env python3
"""Bounded RPG snapshot/replay acceptance; --gpu also exercises the visible player."""
import argparse
import json
import os
from pathlib import Path
import acceptance_process as processes
import tempfile
import time

REPO = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gpu', action='store_true')
    options = parser.parse_args()
    build = ['cargo', 'build', '--locked', '-p', 'titan-cli', '-p', 'titan', '--bin', 'titan',
             '--example', 'procedural_rpg', '--example', 'replay_rpg']
    if options.gpu:
        build += ['--example', 'play_rpg']
    processes.run(build, cwd=REPO, check=True, phase="build")
    metadata = json.loads(processes.check_output(
        ['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=REPO, text=True, phase='build'))
    target = Path(metadata['target_directory'])
    cli = target / 'debug/titan'
    examples = target / 'debug/examples'
    evidence = target / 'rpg-replay-evidence'
    evidence.mkdir(parents=True, exist_ok=True)

    def verify(path):
        result = processes.run([str(examples / 'replay_rpg'), str(path)],
                                capture_output=True, text=True)
        assert result.returncode == 0, (result.stdout, result.stderr)
        return json.loads(result.stdout)

    def scenario(gpu):
        with tempfile.TemporaryDirectory(prefix='titan-rpg-replay-') as directory:
            project = Path(directory).resolve()
            instance = f'rpg-replay-{os.getpid()}-{int(gpu)}'
            command = ([str(examples / 'play_rpg'), '--inspect', '--allow-control'] if gpu else
                       [str(examples / 'procedural_rpg'), '--serve', '--project', str(project), '--allow-mutation'])
            command += ['--assets-dir', str(REPO / 'assets'), '--instance', instance, '--run-for-ms', '30000']
            with tempfile.TemporaryFile(mode='w+') as log:
                process = processes.Popen(command, project=project, instance=instance, cwd=project, stdout=log, stderr=log)
                try:
                    def call(*args, success=True):
                        result = processes.run([str(cli), '--format', 'json', '--project', str(project),
                                                 '--instance', instance, *map(str, args)],
                                                capture_output=True, text=True)
                        response = json.loads(result.stdout)
                        assert response['status'] == ('success' if success else 'failure'), response
                        return response

                    def query(name):
                        return call('query', name)['response']['value']

                    def invoke(name, arguments=None, success=True):
                        return call('invoke', name, '--arguments', json.dumps(arguments or {}), success=success)

                    def status():
                        return call('status')

                    def checksum():
                        return call('capture')['response']['checksum']

                    def entities():
                        return call('entities')['response']['entities']

                    def gameplay_entities():
                        return [entity for entity in entities() if not (entity.get('name') or '').startswith('ui/journal/')]

                    def journal():
                        return query('rpg_state')['journal']

                    def journal_key(key):
                        invoke('journal_key', {'key': key})

                    def ui_text():
                        hud = next(entity['id'] for entity in entities() if entity['name'] == 'ui/quest')
                        detail = call('entity', hud['index'], hud['generation'])['response']['components']
                        return next(value['text'] for key, value in detail.items() if key.endswith('::UiText'))

                    def route(actions):
                        frame = status()['observed_frame']
                        for offset, action in enumerate(actions, 1):
                            call('input', frame + offset, '--actions', json.dumps({action: {'kind': 'button', 'value': True}}))
                        call('step', len(actions))

                    deadline = time.monotonic() + 10
                    while not call('instances')['instances']:
                        assert process.poll() is None and time.monotonic() < deadline, 'RPG did not register'
                        time.sleep(.03)
                    if gpu:
                        invoke('pause')
                        invoke('restart')
                    initial = query('save')
                    initial_checksum = checksum()
                    assert ui_text() == 'SHARDS 0/3'
                    assert journal() == {'open': False, 'selected': 'shards', 'focused': None}
                    before_journal = status()
                    journal_key('toggle')
                    assert journal()['open'] and journal()['focused'] == 'ui/journal/shards'
                    assert status()['response']['paused'] and checksum() != initial_checksum
                    assert query('save') == initial
                    player = next(entity['id'] for entity in entities() if entity['name'] == 'player')
                    components = call('entity', player['index'], player['generation'])['response']['components']
                    position = next(key for key in components if key.endswith('::Position'))
                    for args in [('step', 1), ('input', before_journal['observed_frame'] + 1, '--actions', '{}'),
                                 ('set-field', player['index'], player['generation'], position, 'x', '--value', '0')]:
                        before_rejection = status()
                        rejected = call(*args, success=False)
                        assert (rejected['observed_frame'], rejected['state_revision']) == (before_rejection['observed_frame'], before_rejection['state_revision'])
                        assert query('save') == initial
                    journal_key('next')
                    assert journal()['selected'] == 'shrine' and journal()['focused'] == 'ui/journal/shrine'
                    journal_key('previous')
                    assert journal()['selected'] == 'shards'
                    journal_key('previous')
                    assert journal()['focused'] == 'ui/journal/close'
                    journal_key('activate')
                    assert not journal()['open'] and status()['response']['paused']
                    assert checksum() == initial_checksum
                    assert status()['observed_frame'] == before_journal['observed_frame']
                    # Pointer press/release on the real HUD opens the modal; close uses the ECS button.
                    for pressed in [True, False]:
                        invoke('journal_pointer', {'x': 5, 'y': 5, 'pressed': pressed})
                    assert journal()['open']
                    for pressed in [True, False]:
                        invoke('journal_pointer', {'x': 20, 'y': 88, 'pressed': pressed})
                    assert not journal()['open'] and checksum() == initial_checksum
                    route(['right'] * 2)
                    middle = query('save')
                    middle_checksum = checksum()
                    assert middle['player'] == {'x': 4, 'y': 2}
                    assert middle['collected_shards'] == 1 and not middle['shrine_active']
                    assert ui_text() == 'SHARDS 1/3'
                    assert len(gameplay_entities()) == 5
                    route(['down'] * 3 + ['right'] * 6)
                    final = query('save')
                    assert checksum() == 'f7a298f62ad75c1c'
                    assert ui_text() == 'SHARDS 3/3  SHRINE ACTIVE'
                    assert len(gameplay_entities()) == 3
                    before = status()
                    for invalid in [{}, [], {**middle, 'format_version': 999}]:
                        rejected = invoke('load_save', {'save': invalid}, success=False)
                        assert (rejected['observed_frame'], rejected['state_revision']) == (before['observed_frame'], before['state_revision'])
                        assert query('save') == final and checksum() == 'f7a298f62ad75c1c'
                    invoke('load_save', {'save': middle})
                    assert status()['observed_frame'] == before['observed_frame']
                    assert query('save') == middle and checksum() == middle_checksum
                    assert len(gameplay_entities()) == 5 and ui_text() == 'SHARDS 1/3'
                    route(['down'] * 3 + ['right'] * 6)
                    journal_key('toggle')
                    assert checksum() != 'f7a298f62ad75c1c'
                    recording = query('recording')
                    assert recording['initial_snapshot'] == middle
                    recording_path = evidence / ('gpu-recording.json' if gpu else 'native-recording.json')
                    recording_path.write_text(json.dumps(recording, indent=2) + '\n')
                    verified = verify(recording_path)
                    assert verified['save'] == final == query('save')
                    assert verified['checksum'] == recording['final_checksum'] == 'f7a298f62ad75c1c'
                    assert journal()['open']  # Export must not alter the visible journal.
                    journal_key('close')
                    assert checksum() == verified['checksum']
                    invoke('load_replay', {'recording': recording})
                    assert query('save') == middle and checksum() == middle_checksum
                    replay_frame = status()['observed_frame']
                    journal_key('toggle')
                    assert journal()['open'] and query('rpg_state')['replay']['position'] == 0
                    journal_key('next')
                    journal_key('close')
                    assert status()['observed_frame'] == replay_frame and checksum() == middle_checksum
                    player = next(entity['id'] for entity in entities() if entity['name'] == 'player')
                    components = call('entity', player['index'], player['generation'])['response']['components']
                    position = next(key for key in components if key.endswith('::Position'))
                    for args in [
                        ('step', 10),
                        ('input', replay_frame + 1, '--actions', '{}'),
                        ('set-field', player['index'], player['generation'], position, 'x', '--value', '0'),
                        ('invoke', 'spawn_shard', '--arguments', '{"x":0,"y":0}'),
                        ('invoke', 'load_save', '--arguments', json.dumps({'save': initial})),
                        ('invoke', 'load_replay', '--arguments', '{"recording":{}}'),
                    ]:
                        before = status()
                        rejected = call(*args, success=False)
                        assert (rejected['observed_frame'], rejected['state_revision']) == (before['observed_frame'], before['state_revision'])
                        assert query('save') == middle
                    call('step', 3)
                    assert ui_text() == 'SHARDS 2/3'
                    frame = status()['observed_frame']
                    invoke('restart_replay')
                    assert status()['observed_frame'] == frame and query('save') == middle
                    if gpu:
                        invoke('resume')
                        deadline = time.monotonic() + 3
                        while not status()['response']['paused']:
                            assert time.monotonic() < deadline, query('rpg_state')
                            time.sleep(.03)
                    else:
                        call('step', 9)
                    assert status()['observed_frame'] == frame + 9
                    assert query('rpg_state')['replay']['verified'] is True
                    assert query('save') == final and checksum() == verified['checksum']
                    time.sleep(.1)
                    assert status()['observed_frame'] == frame + 9
                    invoke('stop_replay')
                    assert query('save') == initial and checksum() == initial_checksum
                    # A restored initial snapshot also rebuilds every collected shard.
                    journal_key('toggle')
                    journal_key('next')
                    assert journal()['selected'] == 'shrine'
                    invoke('load_save', {'save': initial})
                    assert not journal()['open'] and journal()['selected'] == 'shards'
                    assert query('save') == initial and checksum() == initial_checksum
                    assert len(gameplay_entities()) == 6 and ui_text() == 'SHARDS 0/3'
                finally:
                    try:
                        processes.graceful_shutdown(process)
                        log.seek(0)
                        output = log.read()
                        assert process.returncode == 0, output
                        if gpu:
                            assert 'GPU frames' in output, output
                        assert not call('instances')['instances']
                    finally:
                        processes.terminate(process)

    scenario(False)
    if options.gpu:
        scenario(True)
    legacy_arena = REPO / 'games/arena/tests/fixtures/recording-v1.json'
    rejected = processes.run([str(examples / 'replay_rpg'), str(legacy_arena)], capture_output=True, text=True)
    assert rejected.returncode != 0, 'RPG must reject arena recordings'
    print('RPG snapshot/replay acceptance passed' + (' including native GPU playback' if options.gpu else ' (headless)'))


if __name__ == '__main__':
    main()
