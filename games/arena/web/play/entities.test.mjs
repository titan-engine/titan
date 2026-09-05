import assert from 'node:assert/strict';
import test from 'node:test';
import { inspectEntities, entityRow } from './entities.mjs';

test('inspection follows entity pages without filtering away enemies', () => {
  const names = ['player', 'enemy-0', 'enemy-1'];
  const requested = [];
  const snapshot = inspectEntities(request => {
    requested.push(request);
    if (request.type === 'entities') {
      assert.deepEqual(request.query, {});
      const index = request.page.cursor ? 2 : 0;
      return { observed_frame: 42, response: {
        entities: names.slice(index, index + 2).map((name, offset) => ({ id: { index: index + offset, generation: 0 }, name })),
        ...(index === 0 ? { next_cursor: '2' } : {}),
      } };
    }
    return { observed_frame: 42, response: { id: request.entity, name: names[request.entity.index], components: {} } };
  });
  assert.deepEqual(snapshot.entities.map(entity => entity.name), names);
  assert.equal(snapshot.observed_frame, 42);
  assert.equal(snapshot.truncated, false);
  assert.equal(requested.filter(request => request.type === 'entities').length, 2);
});

test('unexposed enemy activity is not mislabeled inactive and unnamed entities remain identifiable', () => {
  const entity = { id: { index: 7, generation: 2 }, components: { 'game::Enemy': null } };
  assert.deepEqual(entityRow(entity), ['Entity 7:2', '7:2', 'Activity unavailable', '—']);
});

test('UI entities expose their own position and button state', () => {
  const entity = { id: { index: 16, generation: 0 }, name: 'ui/restart', components: {
    'titan::ui::UiNode': { x: 4, y: 10, visible: true },
    'titan::ui::UiText': { text: 'R RESTART' },
    'titan::ui::UiButton': { enabled: true },
  } };
  assert.deepEqual(entityRow(entity), ['ui/restart', '16:0', 'UI button · enabled', '(4, 10)']);
  entity.components['titan::ui::UiButton'].enabled = false;
  assert.equal(entityRow(entity)[2], 'UI button · disabled');
  entity.components['titan::ui::UiNode'].visible = false;
  assert.equal(entityRow(entity)[2], 'Hidden UI');
});
