#!/usr/bin/env python3
"""Portable build-policy checks; actual external-copy builds have separate tests."""
import contextlib
import io
import importlib.util
import os
import sys
import subprocess
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
        process_helper = self.engine / "scripts/acceptance_process.mjs"
        process_helper.parent.mkdir(parents=True)
        process_helper.write_text("export const run = true;\n")
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
        with patch.object(titan_build.shutil, "which", return_value=None), patch.object(titan_build.processes, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "wasm-bindgen 0.2.126\n"
            titan_build.browser(self.root, self.metadata, package_name="independent-game", out_name="game_bindings")
        self.assertEqual((self.root / "web/shared/input.mjs").read_text(), self.shared_input.read_text())
        self.assertEqual((self.root / "scripts/acceptance_process.mjs").read_text(), "export const run = true;\n")
        self.assertFalse((self.root / "web/assets").exists())
        self.assertTrue(all(call.kwargs["phase"] == "build" for call in run.call_args_list))
        commands = [call.args[0] for call in run.call_args_list]
        install = next(command for command in commands if command[:2] == ["cargo", "install"])
        self.assertEqual(install[install.index("--version") + 1], "0.2.127")
        self.assertIn(["cargo", "build", "--locked", "--package", "independent-game", "--lib", "--target", "wasm32-unknown-unknown", "--release"], commands)
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
        with patch.object(titan_build.shutil, "which", return_value=None), patch.object(titan_build.processes, "run"):
            titan_build.browser(self.root, self.metadata, package_name="independent-game",
                                out_name="game_bindings", assets_source="art source")
        self.assertEqual(list(destination.iterdir()), [destination / "player.png"])
        self.assertEqual((destination / "player.png").read_bytes(), b"PNG bytes")

    def test_missing_explicit_assets_fail_before_build(self):
        with patch.object(titan_build.processes, "run") as run:
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
        with patch.object(titan_build.shutil, "which", return_value=None), patch.object(titan_build.processes, "run", side_effect=RuntimeError("build failed")):
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
        with patch.object(titan_build.shutil, "which", return_value=None), patch.object(titan_build.processes, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "wasm-bindgen 0.2.127\n"
            titan_build.browser(self.root, self.metadata, package_name="independent-game", out_name="game_bindings")
        commands = [call.args[0] for call in run.call_args_list]
        self.assertFalse(any(command[:2] == ["cargo", "install"] for command in commands))
        self.assertTrue(all(command[0] == str(cached) for command in commands if "--out-dir" in command))

    def test_ambiguous_bindgen_resolution_fails_before_build(self):
        self.metadata["packages"].append({"name": "wasm-bindgen", "version": "0.2.126"})
        with patch.object(titan_build.processes, "run") as run:
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
        with patch.object(titan_build.sys, "platform", "darwin"), patch.object(titan_build.processes, "run") as run, contextlib.redirect_stdout(io.StringIO()) as output:
            build = run.return_value
            build.stdout = "\n".join([json.dumps(unrelated), json.dumps(artifact)])
            build.returncode = 0
            titan_build.macos_app(self.root, self.metadata, ["--name", "Copied Game", "--bundle-id", "dev.example.copy", "--release"])
        self.assertIn("--locked", run.call_args.args[0])
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
        with patch.object(titan_build.sys, "platform", "darwin"), patch.object(titan_build.processes, "run") as run, contextlib.redirect_stdout(io.StringIO()) as output:
            build = run.return_value
            build.stdout = json.dumps(artifact)
            build.returncode = 0
            titan_build.macos_app(self.root, self.metadata, ["--example", "play_rpg", "--name", "Titan RPG"])
        self.assertIn("--example", run.call_args.args[0])
        bundle = Path(output.getvalue().strip())
        relocated = self.root / "Relocated RPG.app"
        bundle.rename(relocated)
        with (relocated / "Contents/Info.plist").open("rb") as file:
            info = plistlib.load(file)
        self.assertEqual(info["CFBundleExecutable"], "play_rpg")
        self.assertEqual((relocated / "Contents/MacOS/play_rpg").read_bytes(), b"example executable")
        self.assertEqual((relocated / "Contents/Resources/assets/player.png").read_bytes(), b"player image")

    def test_invalid_bundle_name_does_not_build(self):
        with patch.object(titan_build.sys, "platform", "darwin"), patch.object(titan_build.processes, "run") as run, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                titan_build.macos_app(self.root, self.metadata, ["--name", "../escape"])
            run.assert_not_called()


class LockedMetadata(unittest.TestCase):
    """Exercise real Cargo resolution without network or the engine's lockfile."""

    def test_existing_stale_and_initialized_consumer_graphs(self):
        repository = Path(__file__).resolve().parent.parent
        loaders = [None, *sorted(repository.glob("games/*/scripts/titan_tools.py")),
                   repository / "starters/minimal/scripts/titan_tools.py"]
        with tempfile.TemporaryDirectory(prefix="titan locked consumer ") as temporary:
            root = Path(temporary)
            (root / "src").mkdir()
            (root / "src/lib.rs").write_text("")
            manifest = root / "Cargo.toml"
            manifest.write_text('[package]\nname = "consumer"\nversion = "0.1.0"\nedition = "2021"\n[workspace]\n')
            environment = dict(os.environ, CARGO_NET_OFFLINE="true")

            def initialize():
                subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root,
                               env=environment, check=True, capture_output=True)

            def metadata(loader):
                if loader is None:
                    code = (f"import sys; sys.path.insert(0, {str(repository / 'scripts')!r}); "
                            f"import titan_build; titan_build.cargo_metadata({str(root)!r})")
                else:
                    code = ("import importlib.util; from pathlib import Path; "
                            f"s = importlib.util.spec_from_file_location('loader', {str(loader)!r}); "
                            "m = importlib.util.module_from_spec(s); s.loader.exec_module(m); "
                            f"m.ROOT = Path({str(root)!r}); m.metadata_bootstrap()")
                return subprocess.run([sys.executable, "-c", code], cwd=root,
                                      env=environment, text=True, capture_output=True)

            for loader in loaders:
                with self.subTest(loader=str(loader)):
                    lock = root / "Cargo.lock"
                    lock.unlink(missing_ok=True)
                    missing = metadata(loader)
                    self.assertNotEqual(missing.returncode, 0)
                    self.assertIn("--locked", missing.stderr)
                    self.assertFalse(lock.exists())
                    initialize()
                    original = lock.read_bytes()
                    unchanged = metadata(loader)
                    self.assertEqual(unchanged.returncode, 0, unchanged.stderr)
                    self.assertEqual(lock.read_bytes(), original)
                    # A package identity edit requires a different lockfile even
                    # though this tiny consumer has no registry dependencies.
                    manifest.write_text(manifest.read_text().replace('0.1.0', '0.2.0'))
                    stale = metadata(loader)
                    self.assertNotEqual(stale.returncode, 0)
                    self.assertIn("--locked", stale.stderr)
                    self.assertEqual(lock.read_bytes(), original)
                    initialize()
                    self.assertNotEqual(lock.read_bytes(), original)
                    refreshed = lock.read_bytes()
                    initialized = metadata(loader)
                    self.assertEqual(initialized.returncode, 0, initialized.stderr)
                    self.assertEqual(lock.read_bytes(), refreshed)
                    manifest.write_text(manifest.read_text().replace('0.2.0', '0.1.0'))


class MetadataBootstrap(unittest.TestCase):
    def test_standalone_bootstraps_bound_pipe_holding_metadata_descendants(self):
        repository = Path(__file__).resolve().parent.parent
        for relative in ["games/arena", "starters/minimal"]:
            with self.subTest(package=relative), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                script = root / "cargo"
                pidfile = root / "descendant.pid"
                script.write_text(f"#!{sys.executable}\n"
                                  "import subprocess, sys\n"
                                  "from pathlib import Path\n"
                                  "child = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(60)'])\n"
                                  f"Path({str(pidfile)!r}).write_text(str(child.pid))\n")
                script.chmod(0o755)
                spec = importlib.util.spec_from_file_location("bootstrap", repository / relative / "scripts/titan_tools.py")
                module = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(module)
                environment = dict(os.environ, PATH=str(root) + os.pathsep + os.environ["PATH"],
                                   TITAN_BUILD_TIMEOUT_SECONDS="0.3")
                with patch.dict(os.environ, environment, clear=True):
                    with self.assertRaisesRegex(RuntimeError, "build phase timed out"):
                        module.metadata_bootstrap()
                pid = pidfile.read_text()
                state = titan_build.processes.run(["ps", "-o", "stat=", "-p", pid], capture_output=True, text=True)
                self.assertTrue(state.returncode != 0 or state.stdout.strip().startswith("Z"), state.stdout)


if __name__ == "__main__":
    unittest.main()
