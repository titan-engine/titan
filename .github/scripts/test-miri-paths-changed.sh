#!/usr/bin/env bash
# Exercise the real Git diff/filter, without changing the caller's checkout.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
cd "$fixture"
git init -q
git config user.name 'Miri filter test'
git config user.email 'miri-filter@example.invalid'
git config commit.gpgsign false
mkdir -p crates/bevy_ptr/src
printf 'original\n' > crates/bevy_ptr/src/lib.rs
git add .
git commit -qm base
base=$(git rev-parse HEAD)

assert_changed() {
  local expected=$1
  local actual
  actual=$(bash "$script_dir/miri-paths-changed.sh" "$base" HEAD)
  if [[ "$actual" != "$expected" ]]; then
    echo "Expected $expected, got $actual: $(git diff --name-only "$base...HEAD")" >&2
    exit 1
  fi
}

assert_changed false
for path in \
  README.md docs/guide.md crates/titan_test/src/lib.rs \
  crates/bevy_render/src/lib.rs \
  crates/bevy_ecs/src/lib.rs crates/bevy_ecs/macros/src/lib.rs \
  crates/bevy_ecs/macro_logic/src/lib.rs crates/bevy_ptr/src/lib.rs \
  crates/bevy_tasks/src/lib.rs crates/bevy_utils/src/lib.rs \
  crates/bevy_platform/src/lib.rs crates/bevy_reflect/derive/src/lib.rs \
  crates/bevy_macro_utils/src/lib.rs crates/bevy_debug_stepping/src/lib.rs \
  Cargo.toml Cargo.lock .cargo/config.toml rust-toolchain.toml \
  .github/workflows/ci.yml .github/workflows/miri.yml \
  .github/actions/install-rust/action.yml \
  .github/scripts/miri-paths-changed.sh \
  .github/scripts/test-miri-paths-changed.sh; do
  git reset --hard -q "$base"
  mkdir -p "$(dirname "$path")"
  printf 'changed\n' > "$path"
  git add .
  git commit -qm "$path"
  case "$path" in
    README.md | docs/* | crates/titan_*/* | crates/bevy_render/*) assert_changed false ;;
    *) assert_changed true ;;
  esac
done

# Deletions and moves out of the dependency tree must not disappear from the diff.
git reset --hard -q "$base"
git rm -q crates/bevy_ptr/src/lib.rs
git commit -qm delete
assert_changed true
git reset --hard -q "$base"
git mv crates/bevy_ptr/src/lib.rs moved.rs
git commit -qm rename
assert_changed true

# NUL-delimited paths handle spaces/newlines, and do not match a lookalike crate.
git reset --hard -q "$base"
mkdir -p crates/bevy_ptr_lookalike
printf 'unrelated\n' > $'crates/bevy_ptr_lookalike/file\nwith spaces.rs'
git add .
git commit -qm unrelated
assert_changed false
mkdir -p crates/bevy_ecs/src
printf 'relevant\n' > $'crates/bevy_ecs/src/file\nwith spaces.rs'
git add .
git commit -qm relevant
assert_changed true

# Base-branch-only ECS changes must not cause a docs-only PR to run Miri.
git checkout -qb base-branch "$base"
printf 'base branch change\n' > crates/bevy_ptr/src/lib.rs
git commit -qam base-change
new_base=$(git rev-parse HEAD)
git checkout -qb docs-branch "$base"
printf 'docs only\n' > README.md
git add .
git commit -qm docs
[[ $(bash "$script_dir/miri-paths-changed.sh" "$new_base" HEAD) == false ]]

# A synthetic merge group must cover every merged PR relative to its base SHA.
git checkout -qb merge-group "$base"
git merge --no-ff -qm merge-docs docs-branch
assert_changed false
git merge --no-ff -qm merge-dependency base-branch
assert_changed true

# Bad refs must fail closed, not produce a successful false result.
if bash "$script_dir/miri-paths-changed.sh" missing-ref HEAD >/dev/null 2>&1; then
  echo 'Missing ref unexpectedly succeeded' >&2
  exit 1
fi
echo 'Miri path filter tests passed'
