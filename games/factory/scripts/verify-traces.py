#!/usr/bin/env python3
"""Check native repair semantics against seven historical browser checkpoints."""
import argparse
import json
from pathlib import Path
import sys
import tempfile

REPO = next(parent for parent in Path(__file__).resolve().parents
            if (parent / 'scripts/acceptance_process.py').is_file()
            and (parent / 'games/factory/Cargo.toml').is_file())
GAME = REPO / 'games/factory'
sys.dont_write_bytecode = True
sys.path.insert(0, str(REPO / 'scripts'))
import acceptance_process as processes

EXCLUDED_FIELDS = ['frame', 'hover', 'preview', 'inspected', 'selection']
FIXTURE = GAME / 'tests/fixtures/repair-browser-checkpoints.json'


def repair_sequence():
    operations = []
    checkpoints = []
    def op(**value):
        operations.append(value)
    def place(x, kind='conveyor'):
        op(op='place', kind=kind, x=x, y=3, facing='E')
    def advance(ticks):
        for _ in range(ticks):
            op(op='advance', ticks=1)
    def mark(name):
        checkpoints.append((len(operations) - 1, name))
    place(1, 'extractor')
    for x in range(2, 10):
        place(x)
    advance(600)
    mark('ore-at-delivery')
    op(op='remove', x=5, y=3)
    place(5, 'processor')
    for x in range(6, 10):
        op(op='remove', x=x, y=3)
        place(x)
    advance(600)
    mark('processor-repair')
    op(op='remove', x=5, y=3)
    mark('processor-removed')
    place(5, 'processor')
    advance(726)
    mark('repaired-complete')
    op(op='restart')
    mark('reset')
    place(1, 'extractor')
    for x in [2, 3, 4, 6, 7, 8, 9]:
        place(x)
    place(5, 'processor')
    op(op='rotate', x=5, y=3)
    advance(600)
    mark('wrong-facing')
    for _ in range(3):
        op(op='rotate', x=5, y=3)
    advance(1206)
    mark('facing-repaired-complete')

    return operations, checkpoints


def semantic(state):
    return {key: value for key, value in state.items() if key not in EXCLUDED_FIELDS}


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def verify(result, operations, checkpoints, expected):
    outcomes = result['outcomes']
    require(len(outcomes) == len(operations) == 3767, 'missing operation boundaries')
    require(len(checkpoints) == len(expected) == 7, 'missing browser checkpoints')
    for row in outcomes:
        require('error' not in row, 'rejected operation')
        state = row['state']
        resident = sum(value in ('ore', 'plate') for tile in state['structures']
                       for value in tile['slots'].values())
        require(state['extracted'] == (resident + state['delivered']
                                      + state['discarded_ore'] + state['discarded_plate']),
                'item conservation failed')
    for (index, name), browser in zip(checkpoints, expected):
        require(name == browser['name'], 'checkpoint order changed')
        require(semantic(outcomes[index]['state']) == browser['state'], name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path,
                        default=REPO / 'target/evidence/factory-repair')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    metadata = processes.run(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1',
         '--manifest-path', str(GAME / 'Cargo.toml')], cwd=REPO,
        capture_output=True, text=True, check=True)
    binary = Path(json.loads(metadata.stdout)['target_directory']) / 'debug/titan-factory'
    operations, checkpoints = repair_sequence()
    fixture = json.loads(FIXTURE.read_text())
    with tempfile.TemporaryDirectory(prefix='factory-repair-') as temporary:
        sequence = Path(temporary) / 'sequence.json'
        sequence.write_text(json.dumps(operations, separators=(',', ':')) + '\n')
        run = processes.run([str(binary), '--sequence', str(sequence)], cwd=REPO,
                            capture_output=True, text=True, check=True)
    verify(json.loads(run.stdout), operations, checkpoints, fixture['snapshots'])
    revision = processes.run(['git', 'rev-parse', 'HEAD'], cwd=REPO,
                             capture_output=True, text=True, check=True).stdout.strip()
    summary = {
        'revision': revision,
        'browser_baseline': fixture['source'],
        'operation_boundaries': len(operations),
        'checkpoints': [name for _, name in checkpoints],
        'all_native_recorded_browser_semantic_states_equal': True,
        'conservation_each_boundary': True,
        'excluded_host_ui_fields': EXCLUDED_FIELDS,
        'verification_boundary': 'Native replay against historical browser states; no fresh GUI run',
    }
    (output / 'native-browser-traces.json').write_text(json.dumps(summary, indent=2) + '\n')
    (output / 'repair-sequence.json').write_text(json.dumps(operations, separators=(',', ':')) + '\n')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
