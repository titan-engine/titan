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
