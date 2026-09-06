export function exampleArguments(name) {
  switch (name) {
    case 'construct': return {op:'place',kind:'conveyor',x:2,y:3,facing:'E'};
    case 'place': return {kind:'conveyor',x:2,y:3,facing:'E'};
    case 'rotate': case 'remove': return {x:2,y:3};
    case 'select': return {kind:'conveyor',facing:'E'};
    case 'advance': return {ticks:60};
    case 'sequence': return {operations:[{op:'place',kind:'extractor',x:1,y:3,facing:'E'}]};
    default: return {};
  }
}
export function parseArguments(text) {
  const value = JSON.parse(text);
  if (!value || Array.isArray(value) || typeof value !== 'object') throw new Error('Command arguments must be a JSON object');
  return value;
}
