#!/usr/bin/env node
// Conservative package selection; no dependency resolution or compilation needed.
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

function full(reason) {
  return { packages: '*', reason };
}

function select({ event, base, head, cwd = process.cwd(), run = execFileSync }) {
  const command = (file, args) => run(file, args, { cwd, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  if (!['pull_request', 'merge_group'].includes(event)) return full(`Full coverage for ${event || 'unspecified event'}`);
  if (!base || !head) return full('Missing diff refs');
  try {
    // Reuse the shared classification policy, including unknown/shared paths.
    const classification = command('bash', ['.github/scripts/ci-paths-changed.sh', base, head]).replace(/\r/g, '').trim();
    if (!/^upstream=(true|false)\ntitan=(true|false)\ndocs=(true|false)$/.test(classification)) return full('Invalid shared classification');
    if (classification.startsWith('upstream=true')) return full('Shared, upstream, unknown path, or invalid diff');
    // No rename detection: both the old and new location must be classified.
    const changed = command('git', ['diff', '--name-only', '--no-renames', '-z', `${base}...${head}`]).split('\0').filter(Boolean);
    if (changed.some(file => path.posix.basename(file) === 'Cargo.toml')) return full('Package/dependency/feature manifest changed');
    if (!classification.includes('titan=true')) return { packages: 'none', reason: 'No applicable Titan changes' };

    const metadata = JSON.parse(command('cargo', ['metadata', '--no-deps', '--format-version', '1']));
    const members = new Set(metadata.workspace_members);
    const packages = metadata.packages.filter(pkg => members.has(pkg.id));
    if (packages.length !== members.size || path.resolve(metadata.workspace_root) !== path.resolve(cwd)) throw new Error('Incomplete workspace metadata');
    const byDirectory = new Map();
    const byName = new Map();
    for (const pkg of packages) {
      const directory = path.dirname(pkg.manifest_path);
      if (byDirectory.has(directory) || byName.has(pkg.name)) throw new Error('Ambiguous workspace packages');
      byDirectory.set(directory, pkg);
      byName.set(pkg.name, pkg);
      if (!Array.isArray(pkg.dependencies)) throw new Error('Missing dependency metadata');
    }
    const titan = packages.filter(pkg => pkg.name.startsWith('titan_'));
    if (!titan.length || titan.some(pkg => !/^titan_[a-z0-9_]+$/.test(pkg.name))) throw new Error('Invalid Titan package names');
    const affected = new Set();
    for (const file of changed) {
      if (!file.startsWith('crates/titan_') && !file.startsWith('demos/')) continue;
      // Longest prefix ensures nested packages do not belong to their parent.
      const absolute = path.resolve(cwd, file);
      const owner = packages.filter(pkg => absolute.startsWith(`${path.dirname(pkg.manifest_path)}${path.sep}`))
        .sort((a, b) => b.manifest_path.length - a.manifest_path.length)[0];
      if (!owner || !owner.name.startsWith('titan_')) return full('Changed Titan path has no known Titan workspace owner');
      affected.add(owner.name);
    }
    if (!affected.size) return full('Titan changes could not be assigned to packages');

    // All declared edges, not only the default-feature resolve graph. Includes
    // renamed, optional, target-specific, normal, build and dev dependencies.
    // This over-approximates every existing all-features/headless test recipe.
    const consumers = new Map(packages.map(pkg => [pkg.name, new Set()]));
    for (const pkg of packages) {
      for (const dep of pkg.dependencies) {
        if (![null, 'normal', 'build', 'dev'].includes(dep.kind)) throw new Error('Unknown dependency kind');
        if (!dep.path) {
          if (byName.has(dep.name)) throw new Error('Workspace dependency without a known path');
          continue;
        }
        const provider = byDirectory.get(path.resolve(dep.path));
        if (!provider) throw new Error('Path dependency outside known workspace');
        consumers.get(provider.name).add(pkg.name);
      }
    }
    for (const name of affected) {
      for (const consumer of consumers.get(name)) affected.add(consumer);
    }
    return {
      packages: titan.filter(pkg => affected.has(pkg.name)).map(pkg => pkg.name).sort().join(' '),
      reason: 'Changed packages and transitive workspace consumers (all declared dependency/feature/target edges)',
    };
  } catch (error) {
    return full(`Classification error: ${error.message.split('\n')[0]}`);
  }
}

if (require.main === module) {
  const result = select({ event: process.env.EVENT_NAME, base: process.env.BASE_SHA, head: process.env.HEAD_SHA });
  if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `packages=${result.packages}\n`);
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY,
    `## Titan test selection\n\nPackages: \`${result.packages}\` (\`*\` = all Titan workspace packages).\n\nReason: ${result.reason}\n`);
  console.log(JSON.stringify(result));
}
module.exports = { select };
