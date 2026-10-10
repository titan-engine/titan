const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { plan } = require('./titan-test-features.cjs');

// Registry-free real Cargo regressions: tree/metadata only, never compilation.
test('CLI routes preserve features for direct, demo, multi-package and transitive selections', t => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'titan-features-'));
  t.after(() => fs.rmSync(cwd, { recursive: true, force: true }));
  const manifests = {
    bevy_ecs: '[features]\ntrack_location = []\n',
    bevy_remote: '[features]\ndefault = ["http"]\nhttp = []\nbevy_render = []\n',
    bevy: '[features]\ntrack_location = ["bevy_ecs/track_location"]\nrender = []\nremote = ["dep:bevy_remote"]\n[dependencies]\nbevy_ecs = { path = "../bevy_ecs" }\nbevy_remote = { path = "../bevy_remote", optional = true }\n',
    titan_leaf: '[dependencies]\nbevy_ecs = { path = "../bevy_ecs" }\n',
    titan_demo: '[features]\ndefault = ["render"]\nrender = ["bevy/render"]\nremote = ["bevy/remote"]\n[dependencies]\nbevy = { path = "../bevy", default-features = false }\n',
    bridge: '[dependencies]\nbevy_ecs = { path = "../bevy_ecs" }\nbevy_remote = { path = "../bevy_remote" }\n',
    titan_transitive: '[dependencies]\nbridge = { path = "../bridge" }\n',
    titan_remote_consumer: '[dependencies]\nbevy_remote = { path = "../bevy_remote" }\n',
    titan_alias: '[dependencies]\naliased_ecs = { package = "bevy_ecs", path = "../bevy_ecs" }\n',
    titan_split: '[dependencies]\nbevy = { path = "../bevy" }\n[build-dependencies]\nbuild_ecs = { package = "bevy_ecs", path = "../bevy_ecs" }\n',
    titan_target: '[dependencies]\nbevy = { path = "../bevy" }\n[target.\'cfg(any())\'.dependencies]\ntarget_ecs = { package = "bevy_ecs", path = "../bevy_ecs" }\n',
    titan_remote_split: '[dependencies]\nbevy = { path = "../bevy", features = ["remote"] }\n[build-dependencies]\nbuild_remote = { package = "bevy_remote", path = "../bevy_remote" }\n',
    titan_none: '',
  };
  fs.writeFileSync(path.join(cwd, 'Cargo.toml'), '[workspace]\nresolver = "2"\nmembers = ["*"]\n');
  for (const [name, body] of Object.entries(manifests)) {
    fs.mkdirSync(path.join(cwd, name, 'src'), { recursive: true });
    fs.writeFileSync(path.join(cwd, name, 'Cargo.toml'), `[package]\nname = "${name}"\nversion = "0.1.0"\nedition = "2021"\n${body}`);
    fs.writeFileSync(path.join(cwd, name, 'src/lib.rs'), '');
    if (name.endsWith('split')) fs.writeFileSync(path.join(cwd, name, 'build.rs'), 'fn main() {}\n');
  }
  const cargo = args => execFileSync('cargo', args, { cwd, encoding: 'utf8' });
  const metadata = JSON.parse(cargo(['metadata', '--no-deps', '--offline', '--format-version', '1']));
  const check = (names, expectedDefault, expectedBenches) => {
    const result = plan(metadata, names);
    assert.equal(result.fallback, false);
    assert.deepEqual(result.default, expectedDefault);
    assert.deepEqual(result.benches, expectedBenches);
    for (const features of [result.default, result.benches]) {
      const tree = cargo(['tree', '--offline', '--edges', 'normal,dev', '--format', '{p} [{f}]',
        ...names.flatMap(name => ['-p', name]), ...(features.length ? ['--features', features.join(',')] : [])]);
      // A successful CLI alone is insufficient: inspect target/runtime features,
      // excluding the separately unified build-dependency graph.
      for (const [target, feature] of [['bevy_ecs', 'track_location'], ['bevy_remote', 'bevy_render']]) {
        if (!features.some(route => route.endsWith(`/${feature}`))) continue;
        const lines = tree.split('\n').filter(line => line.includes(`${target} v`));
        assert.ok(lines.length, tree);
        assert.ok(lines.every(line => line.includes(feature)), tree);
      }
    }
  };
  check(['titan_leaf'], ['bevy_ecs/track_location'], []);
  check(['titan_demo'], ['bevy/track_location'], []);
  check(['titan_leaf', 'titan_remote_consumer'], ['bevy_ecs/track_location', 'bevy_remote/bevy_render'], ['bevy_remote/bevy_render']);
  check(['titan_alias'], ['aliased_ecs/track_location'], []);
  check(['titan_none'], [], []);
  check(['titan_split'], ['bevy/track_location'], []);
  check(['titan_target'], ['bevy/track_location'], []);
  assert.equal(plan(metadata, ['titan_remote_split']).fallback, true);
  check(['titan_remote_split', 'titan_remote_consumer'], ['bevy/track_location', 'bevy_remote/bevy_render'], ['bevy_remote/bevy_render']);
  assert.equal(plan(metadata, ['titan_transitive']).fallback, true);
  check(['titan_transitive', 'titan_leaf', 'titan_remote_consumer'], ['bevy_ecs/track_location', 'bevy_remote/bevy_render'], ['bevy_remote/bevy_render']);
  // Optional remote forwarding activated through default features cannot be lost.
  metadata.packages.find(pkg => pkg.name === 'titan_demo').features.default.push('remote');
  assert.equal(plan(metadata, ['titan_demo']).fallback, true);
});

test('weak/renamed optional forwarding, dev/build and target-specific edges', () => {
  const dep = (name, extra = {}) => ({ name, kind: null, optional: false, uses_default_features: false, features: [], ...extra });
  const metadata = { packages: [
    { name: 'titan_root', dependencies: [dep('bevy_ecs', { kind: 'build', target: 'cfg(windows)' }), dep('bridge', { optional: true, rename: 'alias' })],
      features: { default: ['weak'], weak: ['alias?/remote'], enable: ['dep:alias'] } },
    { name: 'bridge', dependencies: [dep('bevy_remote', { optional: true })], features: { remote: ['dep:bevy_remote'] } },
    { name: 'bevy_ecs', dependencies: [], features: { track_location: [] } },
    { name: 'bevy_remote', dependencies: [], features: { bevy_render: [] } },
  ] };
  assert.equal(plan(metadata, ['titan_root']).fallback, true);
  metadata.packages[0].dependencies.push(dep('bevy_ecs', { rename: 'runtime_ecs' }));
  assert.deepEqual(plan(metadata, ['titan_root']).default, ['runtime_ecs/track_location']);
  metadata.packages[0].features.default.push('enable');
  assert.equal(plan(metadata, ['titan_root']).fallback, true);
  metadata.packages[0].dependencies.push(dep('bevy_remote', { kind: 'dev' }));
  assert.deepEqual(plan(metadata, ['titan_root']).benches, ['bevy_remote/bevy_render']);
});
