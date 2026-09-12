"""Exercise the policy checker against real, temporary Cargo workspaces."""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


CHECKER = Path(__file__).with_name("check-dependencies.py")


class DependencyPolicyTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="titan-dependencies-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "workspace"
        self.package(self.root, "game")
        self.package(self.root / "crates" / "internal", "internal")
        self.manifest = (self.root / "Cargo.toml").read_text() + (
            '\n[workspace]\nmembers = ["crates/*"]\nexclude = ["crates/excluded"]\n'
        )

    def package(self, path, name):
        (path / "src").mkdir(parents=True)
        (path / "src" / "lib.rs").write_text("")
        (path / "Cargo.toml").write_text(
            f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2024"\n'
        )

    def check(self, declarations):
        (self.root / "Cargo.toml").write_text(self.manifest + declarations)
        return subprocess.run(
            [sys.executable, str(CHECKER)],
            cwd=self.root,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_internal_paths_and_workspace_inheritance(self):
        # Renaming and version requirements must not turn a local crate into
        # a third-party dependency. Inherited paths are relative to the root.
        (self.root / "crates" / "internal" / "Cargo.toml").write_text(
            (self.root / "crates" / "internal" / "Cargo.toml").read_text()
            + '\n[dependencies]\nshared = { workspace = true }\n'
        )
        self.package(self.root / "crates" / "shared", "shared")
        result = self.check('''
[workspace.dependencies]
shared = { path = "crates/shared" }
[dependencies]
renamed = { package = "internal", path = "crates/internal", version = "0.1.0", optional = true }
[dev-dependencies]
shared = { workspace = true }
[build-dependencies]
shared = { path = "crates/shared" }
[target.'cfg(target_os = "windows")'.dependencies]
shared = { workspace = true }
''')
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_third_party_declarations_are_checked_even_when_unused(self):
        result = self.check('''
[workspace.dependencies]
unused_registry = "999.0"
[dependencies]
optional_registry = { version = "999.0", optional = true }
[dev-dependencies]
dev_registry = "999.0"
[build-dependencies]
build_registry = "999.0"
[target.'cfg(target_os = "windows")'.dependencies]
nonhost_git = { git = "https://example.invalid/dependency.git" }
''')
        self.assertNotEqual(result.returncode, 0)
        for name in ["unused_registry", "optional_registry", "dev_registry", "build_registry", "nonhost_git"]:
            self.assertIn(name, result.stderr)

    def test_local_path_alone_does_not_make_a_dependency_internal(self):
        self.package(self.root / "crates" / "excluded", "excluded")
        self.package(self.root.parent / "outside", "outside")
        for name, path in [("excluded", "crates/excluded"), ("outside", "../outside")]:
            with self.subTest(name=name):
                result = self.check(f'\n[dependencies]\n{name} = {{ path = "{path}" }}\n')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(name, result.stderr)

    def test_internal_crates_cannot_hide_third_party_dependencies(self):
        internal_manifest = self.root / "crates" / "internal" / "Cargo.toml"
        internal_manifest.write_text(
            internal_manifest.read_text() + '\n[dependencies]\nhidden_registry = "999.0"\n'
        )
        result = self.check('\n[dependencies]\ninternal = { path = "crates/internal" }\n')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("hidden_registry", result.stderr)


if __name__ == "__main__":
    unittest.main()
