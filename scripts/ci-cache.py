#!/usr/bin/env python3
"""Identify bounded build caches without caching runtime or discovery artifacts."""
import datetime
import hashlib
import os
from pathlib import Path
import subprocess


GAMES = {'collection-room', 'adventure', 'arena', 'factory'}


def cache_paths(platform, workload):
    allowed = {'workspace', 'bundles', 'adventure', 'arena', 'factory'} if platform == 'macos' else GAMES | {'workspace', 'starter'}
    if platform not in {'native', 'wasm', 'macos'} or workload not in allowed:
        raise ValueError(f'unknown CI workload: {platform}/{workload}')
    # Download archives are small and Cargo unpacks them as needed. Do not store
    # duplicate registry/src trees, incremental state, or the old combined cache.
    paths = ['~/.cargo/registry/index', '~/.cargo/registry/cache', '~/.cargo/git/db']
    roots = ['target']
    if workload in GAMES:
        roots.append(f'games/{workload}/target')
    elif workload == 'starter':
        roots.append('target/starter-smoke')
    elif workload == 'bundles':
        roots = ['target/macos-bundle-smoke']
    for root in roots:
        paths.append(f'{root}/debug')
        if platform == 'wasm':
            # Browser helpers compile release WASM and host proc macros; checks,
            # 3D fixtures and native-agreement tests also use debug profiles.
            paths.extend(f'{root}/{profile}' for profile in [
                'release', 'wasm32-unknown-unknown/debug',
                'wasm32-unknown-unknown/release', 'titan/tools'])
    return paths


def cache_identity(environment, compiler, today):
    # Build mode is part of the identity. An image/toolchain change is cold;
    # dependency changes are cold rather than accumulating unrelated old builds.
    settings = '\n'.join(environment.get(key, '') for key in [
        'ImageOS', 'CARGO_INCREMENTAL', 'CARGO_PROFILE_DEV_DEBUG',
        'CARGO_PROFILE_TEST_DEBUG'])
    version = hashlib.sha256(compiler + settings.encode()).hexdigest()
    prefix = '-'.join(['rust-v2', environment['RUNNER_OS'], environment['RUNNER_ARCH'],
                       environment['CI_PLATFORM'], environment['CI_WORKLOAD'],
                       version, environment['CI_MANIFESTS']]) + '-'
    return prefix + today.isoformat(), prefix


def main():
    paths = cache_paths(os.environ['CI_PLATFORM'], os.environ['CI_WORKLOAD'])
    key, prefix = cache_identity(os.environ, subprocess.check_output(['rustc', '-vV']),
                                 datetime.datetime.now(datetime.timezone.utc).date())
    with Path(os.environ['GITHUB_OUTPUT']).open('a') as output:
        output.write(f'key={key}\nrestore-prefix={prefix}\npaths<<CACHE_PATHS\n')
        output.write('\n'.join(paths) + '\nCACHE_PATHS\n')


if __name__ == '__main__':
    main()
