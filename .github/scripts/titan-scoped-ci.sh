#!/usr/bin/env bash
# Keep Titan's fast path equivalent to the workspace commands for Titan packages.
# Lints/docs retain full selection. Tests accept the conservative classifier result.
set -euo pipefail
mode=${1:?usage: titan-scoped-ci.sh lints|doc|test|extra [packages]}
selection=${2:-'*'}
if [[ ( $mode == test || $mode == extra ) && $selection == none ]]; then
  echo 'No applicable Titan tests.'
  exit 0
fi
# Native Windows jq writes CRLF even when invoked from Git Bash. Normalize the
# text boundary before read builds package arguments; JSON itself accepts CRLF.
crates=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name | startswith("titan_")) | .name' \
  | tr -d '\r' | sort)
if [[ -z "$crates" ]]; then
  echo '::error::No titan_* workspace crates found' >&2
  exit 1
fi
if [[ ( $mode == test || $mode == extra ) && $selection != '*' ]]; then
  # Reject malformed/stale selection by running everything, never by skipping.
  valid=true
  if [[ ! $selection =~ ^titan_[a-z0-9_]+(\ titan_[a-z0-9_]+)*$ ]]; then
    valid=false
  else
    for selected in $selection; do
      if ! grep -Fxq "$selected" <<< "$crates"; then valid=false; fi
    done
  fi
  if [[ $valid == true ]]; then
    crates=$(printf '%s\n' $selection | sort -u)
  else
    echo '::warning::Invalid Titan package selection; running all Titan packages'
  fi
fi
summary() {
  echo "$*"
  if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then printf '%s\n' "$*" >> "$GITHUB_STEP_SUMMARY"; fi
}
summary "## Titan $mode recipes"
summary "Packages: $(echo "$crates" | tr '\n' ' ')"
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
    summary 'Default features: --lib --bins --tests; bevy_ecs/track_location,bevy_remote/bevy_render'
    cargo test "${packages[@]}" --lib --bins --tests --features bevy_ecs/track_location,bevy_remote/bevy_render
    if [[ ${RUNNER_OS:-$(uname -s)} == Linux ]]; then
      # Match tools/ci's Linux benchmark smoke run. The full workspace unifies
      # render implicitly; a separate scoped invocation needs it explicitly.
      # Windows/macOS keep the full driver's --skip-benches behavior.
      summary 'Linux benchmark smoke: --benches; bevy_remote/bevy_render'
      cargo test "${packages[@]}" --benches "${features[@]}"
    fi
    ;;
  extra)
    while IFS= read -r crate; do
      test_args=(test -p "$crate" --all-features)
      case "$crate" in
        titan_remote|titan_mcp)
          # Match workspace render unification while keeping tests GPU-free.
          test_args+=(--features bevy_remote/bevy_render)
          ;;
        titan_doom|titan_puzzle)
          summary "$crate: headless tests and clippy (--no-default-features, --all-targets for clippy)"
          echo "::group::Test and lint $crate (headless)"
          cargo test -p "$crate" --no-default-features
          cargo clippy -p "$crate" --no-default-features --all-targets -- -D warnings
          echo '::endgroup::'
          ;;
      esac
      summary "$crate: ${test_args[*]}"
      echo "::group::Test $crate (all features)"
      cargo "${test_args[@]}"
      echo '::endgroup::'
    done <<< "$crates"
    ;;
  *)
    echo "Unknown scoped CI mode: $mode" >&2
    exit 1
    ;;
esac
