#!/usr/bin/env python3
"""Verify that public packaging excludes unrelated files and fails safely."""
import contextlib
import importlib.util
import io
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("build_pages", Path(__file__).with_name("build-pages.py"))
pages = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pages)


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
        for name in sources:
            source = self.root / name
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_text(f"public fixture: {name}")
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
        self.stage()
        self.assertTrue((self.out / "rpg/inspector/pkg/titan_browser_bg.wasm").is_file())
        self.assertTrue((self.out / "arena/inspector/pkg/titan_game_bg.wasm").is_file())
        self.assertTrue((self.out / ".nojekyll").is_file())
        self.assertFalse(list(self.out.rglob("*.json")))
        old = self.out / "stale.txt"
        old.write_text("stale")
        self.stage()
        self.assertFalse(old.exists())
        self.assertTrue(private.is_file())

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
