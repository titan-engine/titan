#!/usr/bin/env bash
# Print job-selection outputs for the PR/merge-group diff. Unknown paths run full CI.
set -euo pipefail

full() {
  printf 'upstream=true\ntitan=true\ndocs=true\n'
}

# A missing base/head is uncertainty, not an empty change.
if [[ $# != 2 || -z ${1:-} || -z ${2:-} ]]; then
  full
  exit 0
fi
changed_files=$(mktemp)
trap 'rm -f "$changed_files"' EXIT

# Three-dot excludes unrelated base-branch commits. No rename detection ensures
# moves/deletions out of upstream areas still trigger coverage. NULs preserve paths.
if ! git diff --name-only --no-renames -z "$1...$2" > "$changed_files"; then
  full
  exit 0
fi

upstream=false
titan=false
docs=false
while IFS= read -r -d '' path; do
  case "$path" in
    # Check shared/upstream areas BEFORE Markdown: their docs can be generated
    # or included by Rust and need the same coverage as the surrounding code.
    .github/* | .cargo/* | Cargo.toml | Cargo.lock | docs/cargo_features.md | \
    crates/bevy_*/* | \
    src/* | examples/* | benches/* | tools/* | errors/* | tests/* | \
    tests-integration/* | _release-content/* | release-content/*)
      upstream=true
      ;;
    crates/titan_*/* | demos/*)
      titan=true
      ;;
    docs/* | *.md)
      docs=true
      ;;
    *)
      upstream=true
      ;;
  esac
done < "$changed_files"

# Upstream/configuration changes can also affect Titan consumers.
if [[ $upstream == true ]]; then
  full
else
  printf 'upstream=%s\ntitan=%s\ndocs=%s\n' "$upstream" "$titan" "$docs"
fi
