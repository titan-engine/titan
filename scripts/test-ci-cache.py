#!/usr/bin/env python3
"""Guard cache isolation, profile coverage, and immutable refresh boundaries."""
import datetime
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('ci_cache', Path(__file__).with_name('ci-cache.py'))
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)


class CacheTests(unittest.TestCase):
    def test_copied_targets_stay_independent(self):
        paths = cache.cache_paths('wasm', 'starter')
        for root in ['target', 'target/starter-smoke']:
            for profile in ['debug', 'release', 'wasm32-unknown-unknown/debug', 'wasm32-unknown-unknown/release', 'titan/tools']:
                self.assertIn(f'{root}/{profile}', paths)
        self.assertIn('target/macos-bundle-smoke/debug', cache.cache_paths('macos', 'bundles'))

    def test_game_isolation_and_runtime_exclusion(self):
        for platform in ['native', 'wasm', 'macos']:
            for game in cache.GAMES - ({'collection-room'} if platform == 'macos' else set()):
                paths = cache.cache_paths(platform, game)
                self.assertIn(f'games/{game}/target/debug', paths)
                for other in cache.GAMES - {game}:
                    self.assertFalse(any(f'games/{other}/' in path for path in paths))
                self.assertTrue(all('/titan/' not in p or p.endswith('/titan/tools') for p in paths))
                self.assertFalse(any(p.endswith('/target') or p == 'target' for p in paths))
        with self.assertRaises(ValueError):
            cache.cache_paths('native', 'typo')

    def test_compiler_fixtures_have_bounded_output_paths(self):
        paths = cache.cache_paths('native', 'workspace')
        self.assertIn('target/tests/trybuild/debug', paths)
        self.assertIn('target/tests/trybuild/*/debug', paths)
        self.assertNotIn('target/tests/trybuild', paths)
        self.assertFalse(any('trybuild' in p for p in cache.cache_paths('native', 'arena')))

    def test_generation_and_graph_boundaries(self):
        env = dict(RUNNER_OS='Linux', RUNNER_ARCH='X64', CI_PLATFORM='native', CI_WORKLOAD='arena', CI_MANIFESTS='manifest', ImageOS='ubuntu24', CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
        day = datetime.date(2026, 9, 6)
        key, prefix = cache.cache_identity(env, b'rustc pinned', day)
        next_key, next_prefix = cache.cache_identity(env, b'rustc pinned', day + datetime.timedelta(days=1))
        self.assertNotEqual(key, next_key)
        self.assertEqual(prefix, next_prefix)
        self.assertTrue(key.startswith(prefix))
        for field in ['RUNNER_OS', 'RUNNER_ARCH', 'CI_PLATFORM', 'CI_WORKLOAD', 'CI_MANIFESTS', 'ImageOS', 'CARGO_PROFILE_DEV_DEBUG']:
            changed = dict(env, **{field: 'changed'})
            self.assertNotEqual(prefix, cache.cache_identity(changed, b'rustc pinned', day)[1])
        self.assertNotEqual(prefix, cache.cache_identity(env, b'rustc updated', day)[1])


if __name__ == '__main__':
    unittest.main()
