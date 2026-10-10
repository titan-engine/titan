#!/usr/bin/env bash
# Print whether the change from base to head can affect the ECS Miri tests.
set -euo pipefail

base=${1:?usage: miri-paths-changed.sh BASE HEAD}
head=${2:?usage: miri-paths-changed.sh BASE HEAD}
changed_files=$(mktemp)
trap 'rm -f "$changed_files"' EXIT

# Use the PR's merge base, not unrelated changes made on the base branch.
# For merge groups the supplied base SHA is already an ancestor of the head.
# Disable rename detection so moving a file out of a relevant path still counts.
# Write the diff first so a missing ref fails instead of silently skipping Miri.
git diff --name-only --no-renames -z "$base...$head" > "$changed_files"
while IFS= read -r -d '' path; do
  case "$path" in
    # Workspace dependencies from cargo tree -p bevy_ecs --edges normal --prefix none.
    # Nested proc-macro crates are covered by their parent directories.
    crates/bevy_ecs/* | crates/bevy_ptr/* | crates/bevy_tasks/* | \
    crates/bevy_utils/* | crates/bevy_platform/* | crates/bevy_reflect/* | \
    crates/bevy_macro_utils/* | crates/bevy_debug_stepping/* | \
    Cargo.toml | Cargo.lock | .cargo/* | rust-toolchain* | \
    .github/workflows/ci.yml | .github/workflows/miri.yml | \
    .github/actions/install-rust/* | .github/scripts/*miri*.sh)
      echo true
      exit 0
      ;;
  esac
done < "$changed_files"
echo false
