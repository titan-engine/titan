// Read the same scene as the player, including pooled/inactive entities.
export function inspectEntities(request) {
  const entities = [];
  let cursor;
  let observedFrame;
  do {
    const page = request({ type: 'entities', query: {}, page: { limit: 100, ...(cursor ? { cursor } : {}) } });
    observedFrame ??= page.observed_frame;
    for (const summary of page.response.entities) {
      const details = request({ type: 'entity', entity: summary.id });
      entities.push(details.response);
    }
    cursor = page.response.next_cursor;
  } while (cursor && entities.length < 1000);
  return { entities, observed_frame: observedFrame, truncated: Boolean(cursor) };
}

export function entityRow(entity) {
  const entries = Object.entries(entity.components);
  const enemy = entries.find(([name]) => name.endsWith('::Enemy'));
  const player = entries.some(([name]) => name.endsWith('::Player'));
  const node = entries.find(([name]) => name.endsWith('::UiNode'))?.[1];
  const button = entries.find(([name]) => name.endsWith('::UiButton'))?.[1];
  const position = entries.find(([name]) => name.endsWith('::Position'))?.[1] ?? node;
  const activity = enemy
    ? (enemy[1]?.active === true ? 'Active pursuer' : enemy[1]?.active === false ? 'Inactive · awaiting spawn' : 'Activity unavailable')
    : player ? 'Player'
      : node ? (node.visible === false ? 'Hidden UI' : button ? `UI button · ${button.enabled ? 'enabled' : 'disabled'}` : 'UI label')
        : 'Entity';
  return [
    entity.name ?? `Entity ${entity.id.index}:${entity.id.generation}`,
    `${entity.id.index}:${entity.id.generation}`,
    activity,
    position && Number.isFinite(position.x) && Number.isFinite(position.y) ? `(${position.x}, ${position.y})` : '—',
  ];
}
