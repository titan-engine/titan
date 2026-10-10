#!/usr/bin/env bash
# Keep Titan's fast path equivalent to the workspace commands for Titan packages.
# Use all Titan workspace members, not just changed ones: they consume each other.
set -euo pipefail
mode=${1:?usage: titan-scoped-ci.sh lints|doc|test}
# Native Windows jq writes CRLF even when invoked from Git Bash. Normalize the
# text boundary before read builds package arguments; JSON itself accepts CRLF.
crates=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name | startswith("titan_")) | .name' \
  | tr -d '\r' | sort)
if [[ -z "$crates" ]]; then
  echo '::error::No titan_* workspace crates found' >&2
  exit 1
fi
packages=()
while IFS= read -r crate; do
  packages+=(-p "$crate")
done <<< "$crates"

# Match workspace feature unification for bevy_remote's http server. Without
# render, the upstream http-only combination produces dead-code warnings.
features=(--features bevy_remote/bevy_render)
case "$mode" in
  lints)
    cargo fmt --all -- --check
    cargo clippy "${packages[@]}" --all-targets --all-features "${features[@]}" -- -D warnings
    ;;
  doc)
    cargo test "${packages[@]}" --doc "${features[@]}"
    RUSTDOCFLAGS='-D warnings' cargo doc "${packages[@]}" --all-features \
      "${features[@]}" --no-deps --document-private-items --keep-going
    ;;
  test)
    cargo test "${packages[@]}" --lib --bins --tests --features bevy_ecs/track_location,bevy_remote/bevy_render
    ;;
  *)
    echo "Unknown scoped CI mode: $mode" >&2
    exit 1
    ;;
esac
