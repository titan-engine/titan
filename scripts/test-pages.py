#!/usr/bin/env python3
"""Verify that public packaging excludes unrelated files and fails safely."""
import contextlib
import importlib.util
import io
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from urllib.parse import urljoin, urlparse
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("build_pages", Path(__file__).with_name("build-pages.py"))
pages = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pages)
CHECKOUT = pages.ROOT
COLLECTION_ROOM_PUBLIC = {
    "play/index.html", "play/play.js", "play/keys.mjs",
    "inspector/pkg/titan_game.js", "inspector/pkg/titan_game_bg.wasm",
}


class PublicPackaging(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="titan-pages-test-")
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name) / "checkout"
        self.root.mkdir()
        self.out = self.root / "target/pages"
        sources = ["docs/site/index.html", "LICENSE-MIT", "LICENSE-APACHE"]
        sources += [f"web/{path}" for path in pages.RPG]
        sources += [f"games/arena/web/{path}" for path in pages.ARENA]
        sources += [f"games/collection-room/web/{path}" for path in COLLECTION_ROOM_PUBLIC]
        for name in sources:
            source = self.root / name
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_text(f"public fixture: {name}")
        # Use the actual player sources so a newly introduced dependency must be
        # explicitly added to the publication allowlist. Builds are not required.
        for path in ("play/index.html", "play/play.js", "play/keys.mjs"):
            relative = f"games/collection-room/web/{path}"
            shutil.copyfile(CHECKOUT / relative, self.root / relative)
        (self.root / "games/collection-room/web/inspector/pkg/titan_game.js").write_text(
            "const wasm = new URL('titan_game_bg.wasm', import.meta.url);")
        self.patch = patch.object(pages, "ROOT", self.root)
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def stage(self):
        with contextlib.redirect_stdout(io.StringIO()):
            pages.main(["--no-build"])

    def output(self):
        return {str(path.relative_to(self.out)): path.read_bytes()
                for path in self.out.rglob("*") if path.is_file()}

    def test_only_public_assets_and_stale_output_replaced(self):
        private = self.root / "web/inspector/private-registration.json"
        private.write_text("test-only fake private data")
        for relative in ("play/test.html", "play/test.mjs", "play/keys.test.mjs",
                         "inspector/index.html", "inspector/private-registration.json",
                         "inspector/pkg/package.json", "shared/input.mjs"):
            extra = self.root / "games/collection-room/web" / relative
            extra.parent.mkdir(parents=True, exist_ok=True)
            extra.write_text("not a public player dependency")
        self.stage()
        self.assertTrue((self.out / "rpg/inspector/pkg/titan_browser_bg.wasm").is_file())
        self.assertTrue((self.out / "arena/inspector/pkg/titan_game_bg.wasm").is_file())
        self.assertTrue((self.out / ".nojekyll").is_file())
        expected = {"index.html", "LICENSE-MIT", "LICENSE-APACHE", ".nojekyll"}
        expected.update(f"rpg/{path}" for path in pages.RPG)
        expected.update(f"arena/{path}" for path in pages.ARENA)
        expected.update(f"collection-room/{path}" for path in COLLECTION_ROOM_PUBLIC)
        self.assertEqual(set(self.output()), expected)
        self.assertFalse(list(self.out.rglob("*.json")))
        old = self.out / "stale.txt"
        old.write_text("stale")
        self.stage()
        self.assertFalse(old.exists())
        self.assertTrue(private.is_file())

    def test_collection_room_transitive_references_under_hosting_prefixes(self):
        self.stage()
        for prefix in ("/pages/", "/titan/"):
            pending = [urljoin(prefix, "collection-room/play/index.html")]
            visited = set()
            while pending:
                url = pending.pop()
                if url in visited:
                    continue
                visited.add(url)
                self.assertTrue(url.startswith(prefix), url)
                path = self.out / url.removeprefix(prefix)
                self.assertTrue(path.is_file(), f"missing dependency: {url}")
                if path.suffix not in (".html", ".js", ".mjs"):
                    continue
                source = path.read_text()
                if path.suffix == ".html":
                    references = re.findall(r'''(?:src|href)=["']([^"']+)["']''', source)
                else:
                    references = re.findall(r'''(?:\bfrom\s+|\bimport(?:\s+|\s*\(\s*))["']([^"']+)["']''', source)
                    references += re.findall(r'''new URL\(["']([^"']+)["'],\s*import.meta.url\)''', source)
                for reference in references:
                    self.assertFalse(urlparse(reference).scheme, reference)
                    pending.append(urljoin(url, reference))
            self.assertEqual(visited, {prefix + "collection-room/" + path
                                       for path in COLLECTION_ROOM_PUBLIC})

    def test_collection_room_missing_and_symlinked_assets_preserve_previous_site(self):
        self.stage()
        previous = self.output()
        for relative in COLLECTION_ROOM_PUBLIC:
            with self.subTest(asset=relative):
                source = self.root / "games/collection-room/web" / relative
                content = source.read_bytes()
                source.unlink()
                with self.assertRaisesRegex(ValueError, "missing public asset"):
                    self.stage()
                self.assertEqual(self.output(), previous)
                source.symlink_to(self.root / "LICENSE-MIT")
                with self.assertRaisesRegex(ValueError, "symlinked publication path"):
                    self.stage()
                self.assertEqual(self.output(), previous)
                source.unlink()
                source.write_bytes(content)
        assets = self.root / "games/collection-room/web/inspector/pkg"
        external = self.root.parent / "external-collection-pkg"
        shutil.move(assets, external)
        assets.symlink_to(external, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symlinked publication path"):
            self.stage()
        self.assertEqual(self.output(), previous)
        self.assertFalse(list((self.root / "target").glob(".pages-*")))

    def test_full_build_invokes_all_games_and_build_failure_preserves_previous_site(self):
        self.stage()
        previous = self.output()
        with patch.object(pages.subprocess, "run") as run:
            with contextlib.redirect_stdout(io.StringIO()):
                pages.main([])
            self.assertEqual([call.args[0][1] for call in run.call_args_list], [
                str(self.root / "scripts/build-browser.py"),
                str(self.root / "games/arena/scripts/build-browser.py"),
                str(self.root / "games/collection-room/scripts/build-browser.py"),
            ])
            for call in run.call_args_list:
                self.assertEqual(call.kwargs, {"cwd": self.root, "check": True})
        with patch.object(pages.subprocess, "run", side_effect=[
                None, None, subprocess.CalledProcessError(1, "collection-room build")]):
            with self.assertRaises(subprocess.CalledProcessError):
                pages.main([])
        self.assertEqual(self.output(), previous)

    def test_source_symlinks_and_missing_assets_preserve_previous_site(self):
        self.stage()
        previous = self.output()
        assets = self.root / "web/assets"
        external = self.root.parent / "external-assets"
        shutil.move(assets, external)
        assets.symlink_to(external, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symlinked publication path"):
            self.stage()
        self.assertEqual(self.output(), previous)
        assets.unlink()
        shutil.move(external, assets)
        player = assets / "player.png"
        player.unlink()
        player.symlink_to(assets / "tree.png")
        with self.assertRaisesRegex(ValueError, "symlinked publication path"):
            self.stage()
        self.assertEqual(self.output(), previous)
        player.unlink()
        with self.assertRaisesRegex(ValueError, "missing public asset"):
            self.stage()
        self.assertEqual(self.output(), previous)

    def test_symlinked_output_parent_is_not_modified(self):
        external = self.root.parent / "external-target"
        (external / "pages").mkdir(parents=True)
        marker = external / "pages/keep.txt"
        marker.write_text("keep")
        (self.root / "target").symlink_to(external, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symlinked publication path"):
            self.stage()
        self.assertEqual(marker.read_text(), "keep")


if __name__ == "__main__":
    unittest.main()
