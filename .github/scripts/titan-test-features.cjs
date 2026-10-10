// Route workspace CI features through dependencies Cargo accepts for -p selections.
// Cargo rejects package/feature syntax for merely transitive dependencies.
function plan(metadata, names) {
  const packages = new Map(metadata.packages.map(pkg => [pkg.name, pkg]));
  const selected = names.map(name => {
    const pkg = packages.get(name);
    if (!pkg) throw new Error(`Unknown selected package ${name}`);
    return pkg;
  });
  const active = new Map();
  const activate = (name, features) => {
    if (!packages.has(name)) return; // External dependencies cannot be our targets.
    if (!active.has(name)) active.set(name, new Set());
    for (const feature of features) active.get(name).add(feature);
  };
  for (const name of names) activate(name, ['default']);
  // Monotone union of declared default features and dependency feature forwarding.
  // Ignore cfg predicates conservatively; dev dependencies apply to test roots.
  let previous;
  const snapshot = () => JSON.stringify([...active].map(([name, features]) => [name, [...features]]));
  do {
    previous = snapshot();
    for (const [name, enabled] of active) {
      const pkg = packages.get(name);
      const dependencies = pkg.dependencies.filter(dep => dep.kind !== 'dev' || names.includes(name));
      const dependencyFeatures = new Map();
      const activated = new Set(dependencies.filter(dep => !dep.optional).map(dep => dep.rename || dep.name));
      const forward = (alias, feature) => {
        if (!dependencyFeatures.has(alias)) dependencyFeatures.set(alias, new Set());
        dependencyFeatures.get(alias).add(feature);
      };
      for (const feature of enabled) {
        for (const token of (pkg.features || {})[feature] || []) {
          if (token.startsWith('dep:')) activated.add(token.slice(4));
          else if (token.includes('/')) {
            const [dependency, forwarded] = token.split('/');
            const weak = dependency.endsWith('?');
            const alias = weak ? dependency.slice(0, -1) : dependency;
            if (!weak) activated.add(alias);
            forward(alias, forwarded);
          } else enabled.add(token);
        }
      }
      for (const dep of dependencies) {
        const alias = dep.rename || dep.name;
        if (activated.has(alias)) activate(dep.name, [
          ...(dep.uses_default_features ? ['default'] : []),
          ...(dep.features || []), ...(dependencyFeatures.get(alias) || []),
        ]);
      }
    }
  } while (previous !== snapshot());

  const route = (target, feature) => {
    for (const pkg of selected) {
      if (pkg.name === target) return `${target}/${feature}`;
      const dependency = pkg.dependencies.find(dep => dep.name === target);
      if (dependency) return `${dependency.rename || dependency.name}/${feature}`;
    }
    // Bevy exposes ECS tracking publicly; demos have only this transitive route.
    if (target === 'bevy_ecs') {
      for (const pkg of selected) {
        const bevy = pkg.dependencies.find(dep => dep.name === 'bevy');
        if (bevy && packages.get('bevy')?.features?.track_location) return `${bevy.rename || bevy.name}/track_location`;
      }
    }
    return null;
  };
  const tracking = active.has('bevy_ecs') ? route('bevy_ecs', 'track_location') : null;
  const render = active.has('bevy_remote') ? route('bevy_remote', 'bevy_render') : null;
  if ((active.has('bevy_ecs') && !tracking) || (active.has('bevy_remote') && !render)) {
    return { fallback: true, reason: 'Required workspace test features have no direct CLI route; running all Titan packages' };
  }
  return { fallback: false, default: [tracking, render].filter(Boolean), benches: [render].filter(Boolean) };
}
if (require.main === module) {
  const fs = require('node:fs');
  const metadata = JSON.parse(fs.readFileSync(0, 'utf8'));
  console.log(JSON.stringify(plan(metadata, process.argv.slice(2))));
}
module.exports = { plan };
