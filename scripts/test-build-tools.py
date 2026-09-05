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

    def test_invalid_bundle_name_does_not_build(self):
        with patch.object(titan_build.sys, "platform", "darwin"), patch.object(titan_build.subprocess, "Popen") as popen, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                titan_build.macos_app(self.root, self.metadata, ["--name", "../escape"])
            popen.assert_not_called()


if __name__ == "__main__":
    unittest.main()
