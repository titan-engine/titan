// Formatting only: causes, capacities and remedies originate in game::interface.
export const directions = {N:'↑',E:'→',S:'↓',W:'←'};
export function shortcut(event) {
  if (event.repeat || event.ctrlKey || event.metaKey || event.altKey) return null;
  const key=event.key.toLowerCase();
  if (['1','2','3'].includes(key)) return {kind:['conveyor','extractor','processor'][Number(key)-1]};
  return {q:'facing',e:'rotate',x:'remove',r:'restart',' ':'pause','.':'step'}[key] ?? null;
}
export function describeResult(value, before, after) {
  const ore=after.discarded_ore-before.discarded_ore, plate=after.discarded_plate-before.discarded_plate;
  if (ore || plate || value?.discarded_ore !== undefined) return `Removed structure. Discarded ${ore} ore and ${plate} plates.`;
  if(value?.structure)value=value.structure;
  if (value?.kind && value.x !== undefined) return `Selected ${value.kind} at (${value.x},${value.y}), facing ${value.facing}.`;
  if(value?.kind)return `Selected ${value.kind}, facing ${value.facing} ${directions[value.facing]}.`;
  if(value?.structure===null)return `Empty tile (${value.x},${value.y}), ${value.terrain ?? 'ground'}.`;
  return 'Done. Inspect a tile to see its current contents and connection.';
}
export function renderDetails(container, tile) {
  container.replaceChildren();
  const add=(tag,text,cls)=>{const el=container.ownerDocument.createElement(tag);el.textContent=text;if(cls)el.className=cls;container.append(el);return el;};
  if(tile?.structure)tile=tile.structure;
  if (!tile) {add('p','Right click a tile to inspect its contents and connection.');return;}
  add('strong',`${tile.kind ?? 'Empty tile'} (${tile.x},${tile.y})${tile.facing ? ` · facing ${tile.facing} ${directions[tile.facing]}`:''}`);
  const e=tile.explanation;
  if(e){add('p',e.label,e.status==='working'?'good':'bad');add('p',e.detail);if(e.remedy)add('p',e.remedy,'muted');}
  if(tile.inventory?.length){add('h3','Bounded inventory');for(const slot of tile.inventory)add('p',`${slot.slot.replaceAll('_',' ')}: ${slot.item ?? 'empty'} (${slot.count}/${slot.capacity})`);}
  if(tile.recipe){add('h3',tile.recipe.label);add('p',`${tile.recipe.elapsed} / ${tile.recipe.total} work ticks`);const p=add('progress','');p.max=tile.recipe.total || 1;p.value=tile.recipe.elapsed;}
  if(tile.input_connections?.length){add('h3','Input connections');for(const c of tile.input_connections)add('p',c.detail);}
  if(tile.connection){const c=tile.connection;add('h3',`Output · ${c.label}`);add('p',c.detail);if(c.remedy)add('p',c.remedy,'muted');}
  if(tile.removal){add('p',`Removal discards ${tile.removal.ore} ore and ${tile.removal.plate} plates.`);}
}
