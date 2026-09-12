#!/usr/bin/env python3
"""Allow Cargo dependencies only between crates in this workspace."""

import json
import subprocess
import sys
import tomllib
from pathlib import Path


def main() -> int:
    result = subprocess.run(
        ["cargo", "+stable", "metadata", "--offline", "--no-deps", "--format-version", "1"],
        capture_output=True,
        text=True,
    )
    if result.returncode:
        print(result.stderr, end="", file=sys.stderr)
        return 1

    metadata = json.loads(result.stdout)
    root = Path(metadata["workspace_root"]).resolve()
    member_ids = set(metadata["workspace_members"])
    members = [package for package in metadata["packages"] if package["id"] in member_ids]
    member_names = {
        Path(package["manifest_path"]).resolve().parent: package["name"]
        for package in members
    }
    violations = []

    def is_member(path: Path, name: str) -> bool:
        resolved = path.resolve()
        return resolved.is_relative_to(root) and member_names.get(resolved) == name

    for package in members:
        manifest_path = Path(package["manifest_path"])
        if not manifest_path.resolve().is_relative_to(root):
            violations.append(f"{manifest_path}: workspace crates must be inside the workspace directory")
        for dependency in package["dependencies"]:
            path = dependency.get("path")
            if dependency["source"] is None and path and is_member(Path(path), dependency["name"]):
                continue
            kind = dependency["kind"] or "normal"
            target = f", target {dependency['target']}" if dependency["target"] else ""
            name = dependency["rename"] or dependency["name"]
            violations.append(
                f"{manifest_path}: {name} ({kind}{target}) must point to a crate in this workspace"
            )

    # Cargo metadata omits unused workspace dependencies and source overrides.
    manifests = {root / "Cargo.toml", *(Path(package["manifest_path"]) for package in members)}
    for manifest_path in sorted(manifests):
        with manifest_path.open("rb") as file:
            manifest = tomllib.load(file)
        for name, dependency in manifest.get("workspace", {}).get("dependencies", {}).items():
            if isinstance(dependency, dict) and isinstance(dependency.get("path"), str):
                path = manifest_path.parent / dependency["path"]
                package_name = dependency.get("package", name)
                if not {"git", "registry", "registry-index"} & dependency.keys() and is_member(path, package_name):
                    continue
            violations.append(
                f"{manifest_path}: workspace.dependencies.{name} must point to a crate in this workspace"
            )
        for registry, dependencies in manifest.get("patch", {}).items():
            for name in dependencies:
                violations.append(f"{manifest_path}: patch.{registry}.{name} is not allowed")
        for name in manifest.get("replace", {}):
            violations.append(f"{manifest_path}: replace.{name} is not allowed")

    if violations:
        print("Only dependencies between crates in this workspace are allowed.", file=sys.stderr)
        for violation in violations:
            print(f"- {violation}", file=sys.stderr)
        return 1

    print(f"Dependency policy passed for {len(members)} workspace package(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
