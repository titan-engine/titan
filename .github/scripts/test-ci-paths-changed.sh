#!/usr/bin/env bash
# Exercise real Git diffs in a disposable repository, not the caller's checkout.
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
cd "$fixture"
git init -q
git config user.name 'CI classifier test'
git config user.email 'ci-classifier@example.invalid'
git config commit.gpgsign false
mkdir -p crates/bevy_ecs/src crates/titan_test/src docs
printf 'generated docs\n' > docs/cargo_features.md
printf 'original\n' > crates/bevy_ecs/src/lib.rs
printf 'original\n' > crates/titan_test/src/lib.rs
git add .
git commit -qm base
base=$(git rev-parse HEAD)
full=$'upstream=true\ntitan=true\ndocs=true'
titan=$'upstream=false\ntitan=true\ndocs=false'
docs=$'upstream=false\ntitan=false\ndocs=true'
empty=$'upstream=false\ntitan=false\ndocs=false'

assert_selection() {
  local expected=$1 actual
  actual=$(bash "$script_dir/ci-paths-changed.sh" "$base" HEAD)
  if [[ "$actual" != "$expected" ]]; then
    printf 'Expected:\n%s\nGot:\n%s\n' "$expected" "$actual" >&2
    git diff --name-only "$base...HEAD" >&2
    exit 1
  fi
}

assert_selection "$empty"
for path in \
  crates/titan_test/src/lib.rs crates/titan_new/src/lib.rs demos/doom/src/main.rs \
  crates/titan_test/README.md demos/doom/Cargo.toml \
  README.md MAINTAINING.md docs/guide.md docs/assets/diagram.svg docs/cargo_features.md \
  crates/bevy_ecs/src/lib.rs crates/bevy_render/README.md src/lib.rs \
  examples/README.md benches/a.rs tools/ci/src/main.rs errors/README.md \
  tests/a.rs tests-integration/a.rs _release-content/notes.md \
  Cargo.toml Cargo.lock .cargo/config.toml rust-toolchain.toml rustfmt.toml \
  deny.toml .github/workflows/ci.yml .github/actions/a/action.yml \
  .github/scripts/a.sh .github/README.md unknown.rs assets/shader.wgsl \
  crates/titan_lookalike.rs crates/not_titan/src/lib.rs; do
  git reset --hard -q "$base"
  mkdir -p "$(dirname "$path")"
  printf 'changed\n' > "$path"
  git add .
  git commit -qm "$path"
  case "$path" in
    crates/titan_*/* | demos/*) assert_selection "$titan" ;;
    docs/cargo_features.md) assert_selection "$full" ;;
    README.md | MAINTAINING.md | docs/*) assert_selection "$docs" ;;
    *) assert_selection "$full" ;;
  esac
done

# Mixed Titan + docs remains scoped, but adding any upstream path runs everything.
git reset --hard -q "$base"
printf 'changed\n' > crates/titan_test/src/lib.rs
printf 'docs\n' > README.md
git add . && git commit -qm mixed
assert_selection $'upstream=false\ntitan=true\ndocs=true'
printf 'upstream\n' > crates/bevy_ecs/src/lib.rs
git commit -qam upstream
assert_selection "$full"

# Generated docs are included by src/lib.rs: deleting them must run Rust/docs CI.
git reset --hard -q "$base"
git rm -q docs/cargo_features.md
git commit -qm generated-docs-deletion
assert_selection "$full"

# Deletions and moves out of an upstream directory still require upstream checks.
git reset --hard -q "$base"
git rm -q crates/bevy_ecs/src/lib.rs
git commit -qm deletion
assert_selection "$full"
git reset --hard -q "$base"
git mv crates/bevy_ecs/src/lib.rs crates/titan_test/src/moved.rs
git commit -qm rename
assert_selection "$full"
git reset --hard -q "$base"
git rm -q crates/titan_test/src/lib.rs
git commit -qm titan-deletion
assert_selection "$titan"

# NUL-delimited names preserve spaces/newlines. Unknown lookalikes fail safe.
git reset --hard -q "$base"
printf 'change\n' > $'crates/titan_test/src/file\nwith spaces.rs'
git add . && git commit -qm unusual-titan
assert_selection "$titan"
mkdir -p crates/bevy_ecs/src
printf 'change\n' > $'crates/bevy_ecs/src/file\nwith spaces.md'
git add . && git commit -qm unusual-upstream
assert_selection "$full"

# Changes made only on the base branch must not expand a Titan-only PR's scope.
git checkout -qb base-branch "$base"
printf 'base change\n' > crates/bevy_ecs/src/lib.rs
git commit -qam base-change
new_base=$(git rev-parse HEAD)
git checkout -qb titan-branch "$base"
printf 'titan change\n' > crates/titan_test/src/lib.rs
git commit -qam titan-change
[[ $(bash "$script_dir/ci-paths-changed.sh" "$new_base" HEAD) == "$titan" ]]

# Merge-group diff covers all entries, not just the last PR.
git checkout -qb merge-group "$base"
git merge --no-ff -qm merge-titan titan-branch
assert_selection "$titan"
git merge --no-ff -qm merge-upstream base-branch
assert_selection "$full"

# Missing refs, no merge base, and omitted arguments must select full coverage.
[[ $(bash "$script_dir/ci-paths-changed.sh" missing-ref HEAD 2>/dev/null) == "$full" ]]
[[ $(bash "$script_dir/ci-paths-changed.sh") == "$full" ]]
[[ $(bash "$script_dir/ci-paths-changed.sh" '' HEAD) == "$full" ]]
git checkout --orphan unrelated -q
git rm -rfq .
printf 'unrelated\n' > README.md
git add . && git commit -qm unrelated
[[ $(bash "$script_dir/ci-paths-changed.sh" "$base" HEAD 2>/dev/null) == "$full" ]]
# Execute the composite action's actual shell body too: non-diff events, script
# failure and malformed output must select full coverage, not only bad Git refs.
# Strip the YAML block's indentation; no YAML library or runner is needed.
awk '
  /^    - name: Select jobs$/ { select_step=1 }
  select_step && /^      run: \|$/ { body=1; next }
  body { sub(/^        /, ""); print }
' "$script_dir/../actions/ci-changes/action.yml" > select-jobs.sh
[[ -s select-jobs.sh ]]
mkdir -p .github/scripts
cp "$script_dir/ci-paths-changed.sh" .github/scripts/
cp "$script_dir/miri-paths-changed.sh" .github/scripts/
assert_action() {
  local event=$1 expected=$2
  : > outputs
  : > summary
  EVENT_NAME=$event BASE_SHA=$base HEAD_SHA=titan-branch \
    GITHUB_OUTPUT="$fixture/outputs" GITHUB_STEP_SUMMARY="$fixture/summary" \
    bash select-jobs.sh
  [[ $(< outputs) == "$expected" ]]
}
assert_action pull_request "$titan"$'\necs=false'
assert_action merge_group "$titan"$'\necs=false'
for event in push schedule workflow_dispatch unknown-event; do
  assert_action "$event" "$full"$'\necs=true'
done
printf 'exit 1\n' > .github/scripts/ci-paths-changed.sh
assert_action pull_request "$full"$'\necs=true'
printf "printf 'upstream=false\\\\ntitan=false\\\\ndocs=invalid\\\\n'\n" > .github/scripts/ci-paths-changed.sh
assert_action merge_group "$full"$'\necs=true'
cp "$script_dir/ci-paths-changed.sh" .github/scripts/
printf 'exit 1\n' > .github/scripts/miri-paths-changed.sh
assert_action pull_request "$titan"$'\necs=true'
echo 'CI classifier and action fail-safe tests passed'
