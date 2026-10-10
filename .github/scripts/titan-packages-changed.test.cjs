const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { select } = require('./titan-packages-changed.cjs');

function fixture(t) {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'titan-selection-'));
  t.after(() => fs.rmSync(cwd, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
  const write = (file, content = 'changed\n') => {
    fs.mkdirSync(path.dirname(path.join(cwd, file)), { recursive: true });
    fs.writeFileSync(path.join(cwd, file), content);
  };
  write('.github/scripts/ci-paths-changed.sh', fs.readFileSync(path.join(__dirname, 'ci-paths-changed.sh')));
  const names = ['titan_leaf', 'titan_normal', 'titan_build', 'titan_dev', 'titan_optional', 'titan_chain', 'titan_unrelated', 'bevy_bridge', 'titan_bridge_consumer', 'titan_doom'];
  const directories = new Map(names.map(name => [name, name === 'titan_doom' ? 'demos/doom' : `crates/${name}`]));
  const packages = names.map(name => ({
    id: name, name, manifest_path: path.join(cwd, directories.get(name), 'Cargo.toml'), dependencies: [],
  }));
  const dep = (consumer, provider, kind = null, extra = {}) => packages.find(pkg => pkg.name === consumer).dependencies.push({
    name: provider, path: path.join(cwd, directories.get(provider)), kind, ...extra,
  });
  dep('titan_normal', 'titan_leaf');
  dep('titan_build', 'titan_leaf', 'build');
  dep('titan_dev', 'titan_leaf', 'dev');
  dep('titan_optional', 'titan_leaf', null, { optional: true, rename: 'alias', target: 'cfg(windows)' });
  dep('titan_chain', 'titan_dev');
  dep('bevy_bridge', 'titan_leaf');
  dep('titan_bridge_consumer', 'bevy_bridge');
  const metadata = { packages, workspace_members: names, workspace_root: cwd };
  for (const directory of directories.values()) write(`${directory}/src/lib.rs`, 'base\n');
  write('Cargo.toml', 'workspace\n');
  write('crates/titan_leaf/Cargo.toml', 'package\n');
  git('init', '-q');
  git('config', 'user.name', 'Titan selection test');
  git('config', 'user.email', 'ci@example.invalid');
  git('config', 'commit.gpgsign', 'false');
  git('add', '.');
  git('commit', '-qm', 'base');
  const base = git('rev-parse', 'HEAD');
  const commit = () => { git('add', '-A'); git('commit', '-qm', 'change'); };
  const run = (file, args, options) => file === 'cargo' ? JSON.stringify(metadata) + '\r\n' : execFileSync(file, args, options);
  const selection = (overrides = {}) => select({ event: 'pull_request', base, head: 'HEAD', cwd, run, ...overrides });
  return { cwd, git, write, metadata, commit, selection, base, run };
}

test('leaf selects only itself; graph includes normal/build/dev/optional/renamed/target edges', t => {
  const f = fixture(t);
  f.write('crates/titan_chain/tests/integration.rs'); f.commit();
  assert.equal(f.selection().packages, 'titan_chain');
  f.metadata.packages.find(pkg => pkg.name === 'bevy_bridge').dependencies = [];
  f.git('reset', '--hard', f.base);
  f.write('crates/titan_leaf/src/lib.rs'); f.commit();
  assert.equal(f.selection().packages, 'titan_build titan_chain titan_dev titan_leaf titan_normal titan_optional');
});

test('non-Titan downstream consumers request upstream/full coverage', t => {
  const f = fixture(t);
  f.write('crates/titan_leaf/src/lib.rs'); f.commit();
  const result = f.selection();
  assert.equal(result.packages, '*');
  assert.equal(result.upstream, true);
  assert.match(result.reason, /non-Titan workspace consumer/);
});

test('multiple changed packages and demo ownership', t => {
  const f = fixture(t);
  f.write('crates/titan_chain/src/lib.rs'); f.write('demos/doom/src/lib.rs'); f.commit();
  assert.equal(f.selection().packages, 'titan_chain titan_doom');
});

test('file additions, deletions and renames include both old/new owners, even with unusual names', t => {
  const f = fixture(t);
  // Native Win32 APIs reject control characters in filenames; the shared Bash
  // classifier fixtures still exercise NUL/newline handling on MSYS and Unix.
  const unusual = process.platform === 'win32' ? 'file with spaces.rs' : 'file\nwith spaces.rs';
  f.write(`crates/titan_chain/src/${unusual}`); f.commit();
  assert.equal(f.selection().packages, 'titan_chain');
  f.git('reset', '--hard', f.base);
  f.git('rm', 'crates/titan_chain/src/lib.rs'); f.commit();
  assert.equal(f.selection().packages, 'titan_chain');
  f.git('reset', '--hard', f.base);
  f.git('mv', 'crates/titan_chain/src/lib.rs', 'crates/titan_unrelated/src/moved.rs'); f.commit();
  assert.equal(f.selection().packages, 'titan_chain titan_unrelated');
  f.git('reset', '--hard', f.base);
  f.git('mv', 'crates/titan_chain/src/lib.rs', 'crates/bevy_bridge/src/moved.rs'); f.commit();
  assert.equal(f.selection().packages, '*');
});

test('manifest additions/deletions/feature edits, shared/root files, upstream, unknown owners/paths fail safe', t => {
  const f = fixture(t);
  for (const file of ['Cargo.toml', 'Cargo.lock', 'crates/titan_leaf/Cargo.toml', 'crates/titan_chain/Cargo.toml',
    'demos/doom/Cargo.toml', 'crates/titan_new/src/lib.rs', 'demos/unknown/data.txt', '.github/workflows/titan.yml',
    '.cargo/config.toml', 'rust-toolchain.toml', 'docs/cargo_features.md', 'crates/bevy_bridge/src/lib.rs', 'assets/a.wgsl', 'unknown.rs']) {
    f.git('reset', '--hard', f.base);
    f.write(file); f.commit();
    assert.equal(f.selection().packages, '*', file);
  }
  f.git('reset', '--hard', f.base);
  f.git('rm', 'crates/titan_leaf/Cargo.toml'); f.commit();
  assert.equal(f.selection().packages, '*');
});

test('docs-only and empty diffs have no applicable tests; non-diff events remain full', t => {
  const f = fixture(t);
  assert.equal(f.selection().packages, 'none');
  f.write('docs/guide.md'); f.commit();
  assert.equal(f.selection().packages, 'none');
  for (const event of ['push', 'schedule', 'workflow_dispatch', 'unknown', undefined]) {
    assert.equal(f.selection({ event }).packages, '*');
  }
});

test('PR three-dot excludes base-only edits; merge-group includes every queued change', t => {
  const f = fixture(t);
  f.git('checkout', '-qb', 'base-branch');
  f.write('crates/bevy_bridge/src/lib.rs'); f.commit();
  const newBase = f.git('rev-parse', 'HEAD');
  f.git('checkout', '-qb', 'pr-branch', f.base);
  f.write('crates/titan_chain/src/lib.rs'); f.commit();
  assert.equal(f.selection({ base: newBase }).packages, 'titan_chain');
  f.git('checkout', '-qb', 'second-pr', f.base);
  f.write('crates/titan_unrelated/src/lib.rs'); f.commit();
  f.git('checkout', '-qb', 'merge-group', f.base);
  f.git('merge', '--no-ff', '-qm', 'queue', 'pr-branch', 'second-pr');
  assert.equal(f.selection({ event: 'merge_group' }).packages, 'titan_chain titan_unrelated');
});

test('invalid/missing refs and unrelated histories select full', t => {
  const f = fixture(t);
  for (const base of ['', undefined, 'missing-ref']) assert.equal(f.selection({ base }).packages, '*');
  f.git('checkout', '--orphan', 'unrelated');
  f.git('rm', '-rf', '.'); f.write('README.md'); f.commit();
  // Keep the producer available so this specifically tests a missing merge base.
  f.write('.github/scripts/ci-paths-changed.sh', fs.readFileSync(path.join(__dirname, 'ci-paths-changed.sh')));
  assert.equal(f.selection().packages, '*');
});

test('classification/metadata failures and malformed/incomplete graph select full; CRLF accepted', t => {
  const f = fixture(t);
  f.write('crates/titan_chain/src/lib.rs'); f.commit();
  assert.equal(f.selection({ run: (file, args, options) => {
    if (file === 'bash') return 'upstream=false\r\ntitan=true\r\ndocs=false\r\n';
    return f.run(file, args, options);
  } }).packages, 'titan_chain');
  for (const result of ['upstream=false\ntitan=true\ndocs=bad', '']) {
    assert.equal(f.selection({ run: () => result }).packages, '*');
  }
  for (const failed of ['bash', 'git', 'cargo']) assert.equal(f.selection({ run: (file, args, options) => {
    if (file === failed) throw new Error('producer failed');
    return f.run(file, args, options);
  } }).packages, '*');
  assert.equal(f.selection({ run: (file, args, options) => file === 'cargo' ? '{broken' : f.run(file, args, options) }).packages, '*');
  f.metadata.packages[0].dependencies.push({ name: 'unknown', kind: null, path: path.join(f.cwd, 'outside') });
  assert.equal(f.selection().packages, '*');
  f.metadata.packages[0].dependencies = [{ name: 'titan_chain', kind: null }];
  assert.equal(f.selection().packages, '*');
  f.metadata.packages[0].dependencies = [];
  f.metadata.workspace_members.push('missing');
  assert.equal(f.selection().packages, '*');
});
