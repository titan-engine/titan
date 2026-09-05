#!/usr/bin/env python3
"""Portable build-policy checks; actual external-copy builds have separate tests."""
import contextlib
import io
import json
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch

import titan_build


class BuildTools(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="titan build policy ")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.target = self.root / "custom target"
        self.engine = self.root / "engine checkout"
        self.shared_input = self.engine / "web/shared/input.mjs"
        self.shared_input.parent.mkdir(parents=True)
        self.shared_input.write_text("export const engineInput = true;\n")
        self.metadata = {
            "target_directory": str(self.target),
            "packages": [
                {"name": "titan", "manifest_path": str(self.engine / "Cargo.toml")},
                {"name": "wasm-bindgen", "version": "0.2.127", "manifest_path": str(self.root / "bindgen/Cargo.toml")},
                {"name": "independent-game", "id": "game-id", "version": "1.2.3-dev",
                 "manifest_path": str(self.root / "Cargo.toml"), "targets": [
                     {"name": "custom-library", "crate_types": ["cdylib", "rlib"], "kind": ["cdylib", "rlib"]},
                     {"name": "play", "crate_types": ["bin"], "kind": ["bin"]}]},
            ],
        }

    def cli(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch()
        return path

    def test_browser_rejects_stale_cli_and_installs_matching_version(self):
        self.cli(self.target / "titan/tools/bin/wasm-bindgen")
        with patch.object(titan_build.shutil, "which", return_value=None), patch.object(titan_build.subprocess, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "wasm-bindgen 0.2.126\n"
            titan_build.browser(self.root, self.metadata, package_name="independent-game", out_name="game_bindings")
        self.assertEqual((self.root / "web/shared/input.mjs").read_text(), self.shared_input.read_text())
        self.assertFalse((self.root / "web/assets").exists())
        commands = [call.args[0] for call in run.call_args_list]
        install = next(command for command in commands if command[:2] == ["cargo", "install"])
        self.assertEqual(install[install.index("--version") + 1], "0.2.127")
        self.assertIn(["cargo", "build", "--package", "independent-game", "--lib", "--target", "wasm32-unknown-unknown", "--release"], commands)
        bindings = [command for command in commands if "--out-dir" in command]
        self.assertEqual(len(bindings), 2)
        self.assertEqual(bindings[0][1], str(self.target / "wasm32-unknown-unknown/release/custom_library.wasm"))
        self.assertEqual(bindings[0][-1], "game_bindings")
        self.assertIn(str(self.root / "web/inspector/pkg"), bindings[0])
        self.assertIn(str(self.target / "titan/browser-node"), bindings[1])

    def test_browser_packages_resources_and_removes_stale_files(self):
        source = self.root / "art source"
        source.mkdir()
        (source / "player.png").write_bytes(b"PNG bytes")
        destination = self.root / "web/assets"
        destination.mkdir(parents=True)
        (destination / "stale.png").write_bytes(b"old asset")
        with patch.object(titan_build.shutil, "which", return_value=None), patch.object(titan_build.subprocess, "run"):
            titan_build.browser(self.root, self.metadata, package_name="independent-game",
                                out_name="game_bindings", assets_source="art source")
        self.assertEqual(list(destination.iterdir()), [destination / "player.png"])
        self.assertEqual((destination / "player.png").read_bytes(), b"PNG bytes")

    def test_missing_explicit_assets_fail_before_build(self):
        with patch.object(titan_build.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "not a directory"):
                titan_build.browser(self.root, self.metadata, package_name="independent-game",
                                    out_name="game_bindings", assets_source="missing")
            run.assert_not_called()

    def test_failed_browser_build_preserves_previous_resources(self):
        source = self.root / "assets"
        source.mkdir()
        (source / "player.png").write_bytes(b"new")
        destination = self.root / "web/assets"
        destination.mkdir(parents=True)
        (destination / "player.png").write_bytes(b"previous successful build")
        with patch.object(titan_build.shutil, "which", return_value=None), patch.object(titan_build.subprocess, "run", side_effect=RuntimeError("build failed")):
            with self.assertRaisesRegex(RuntimeError, "build failed"):
                titan_build.browser(self.root, self.metadata, package_name="independent-game", out_name="game_bindings")
        self.assertEqual((destination / "player.png").read_bytes(), b"previous successful build")

    def test_asset_copy_rejects_overlapping_directories(self):
        source = self.root / "web/assets/source"
        source.mkdir(parents=True)
        (source / "player.png").write_bytes(b"unchanged")
        with self.assertRaisesRegex(ValueError, "outside the source"):
            titan_build.copy_assets(source, source.parent)
        self.assertEqual((source / "player.png").read_bytes(), b"unchanged")

    def test_asset_symlinks_are_rejected(self):
        source = self.root / "assets"
        source.mkdir()
        (source / "outside").symlink_to(self.shared_input)
        with self.assertRaisesRegex(ValueError, "non-regular"):
            titan_build.asset_source(self.root, None)

    def test_browser_reuses_matching_dependency_checkout_cli(self):
        cached = self.cli(self.engine / "target/titan/tools/bin/wasm-bindgen")
        with patch.object(titan_build.shutil, "which", return_value=None), patch.object(titan_build.subprocess, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "wasm-bindgen 0.2.127\n"
            titan_build.browser(self.root, self.metadata, package_name="independent-game", out_name="game_bindings")
        commands = [call.args[0] for call in run.call_args_list]
        self.assertFalse(any(command[:2] == ["cargo", "install"] for command in commands))
        self.assertTrue(all(command[0] == str(cached) for command in commands if "--out-dir" in command))

    def test_ambiguous_bindgen_resolution_fails_before_build(self):
        self.metadata["packages"].append({"name": "wasm-bindgen", "version": "0.2.126"})
        with patch.object(titan_build.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "expected one resolved"):
                titan_build.browser(self.root, self.metadata, package_name="independent-game", out_name="game_bindings")
            run.assert_not_called()

    def test_bundle_uses_reported_executable_and_package_identity(self):
        executable = self.root / "nonstandard-triple" / "player"
        executable.parent.mkdir()
        executable.write_text("game executable")
        executable.chmod(0o755)
        artifact = {"reason": "compiler-artifact", "package_id": "game-id", "target": {"name": "play"}, "executable": str(executable)}
        unrelated = dict(artifact, package_id="dependency-id", executable="/wrong/file")
        with patch.object(titan_build.sys, "platform", "darwin"), patch.object(titan_build.subprocess, "Popen") as popen, contextlib.redirect_stdout(io.StringIO()) as output:
            build = popen.return_value.__enter__.return_value
            build.stdout = [json.dumps(unrelated), json.dumps(artifact)]
            build.wait.return_value = 0
            titan_build.macos_app(self.root, self.metadata, ["--name", "Copied Game", "--bundle-id", "dev.example.copy", "--release"])
        bundle = Path(output.getvalue().strip())
        self.assertEqual(bundle, self.target / "macos-app/release/Copied Game.app")
        with (bundle / "Contents/Info.plist").open("rb") as file:
            info = plistlib.load(file)
        self.assertEqual(info["CFBundleIdentifier"], "dev.example.copy")
        self.assertEqual(info["CFBundleVersion"], "1.2.3")
        self.assertEqual((bundle / "Contents/MacOS/play").read_text(), "game executable")
        self.assertFalse((bundle / "Contents/Resources").exists())

    def test_example_bundle_resources_survive_relocation(self):
        package = self.metadata["packages"][-1]
        package["targets"].append({"name": "play_rpg", "kind": ["example"], "crate_types": ["bin"]})
        executable = self.root / "built-example"
        executable.write_bytes(b"example executable")
        (self.root / "assets").mkdir()
        (self.root / "assets/player.png").write_bytes(b"player image")
        artifact = {"reason": "compiler-artifact", "package_id": "game-id",
                    "target": {"name": "play_rpg"}, "executable": str(executable)}
        with patch.object(titan_build.sys, "platform", "darwin"), patch.object(titan_build.subprocess, "Popen") as popen, contextlib.redirect_stdout(io.StringIO()) as output:
            build = popen.return_value.__enter__.return_value
            build.stdout = [json.dumps(artifact)]
            build.wait.return_value = 0
            titan_build.macos_app(self.root, self.metadata, ["--example", "play_rpg", "--name", "Titan RPG"])
        self.assertIn("--example", popen.call_args.args[0])
        bundle = Path(output.getvalue().strip())
        relocated = self.root / "Relocated RPG.app"
        bundle.rename(relocated)
        with (relocated / "Contents/Info.plist").open("rb") as file:
            info = plistlib.load(file)
        self.assertEqual(info["CFBundleExecutable"], "play_rpg")
        self.assertEqual((relocated / "Contents/MacOS/play_rpg").read_bytes(), b"example executable")
        self.assertEqual((relocated / "Contents/Resources/assets/player.png").read_bytes(), b"player image")

    def test_invalid_bundle_name_does_not_build(self):
        with patch.object(titan_build.sys, "platform", "darwin"), patch.object(titan_build.subprocess, "Popen") as popen, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                titan_build.macos_app(self.root, self.metadata, ["--name", "../escape"])
            popen.assert_not_called()


if __name__ == "__main__":
    unittest.main()
