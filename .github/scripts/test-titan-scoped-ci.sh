#!/usr/bin/env bash
# Verify scoped command construction without compiling the engine.
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin"
export COMMAND_LOG="$fixture/commands"
export PATH="$fixture/bin:$PATH"
cat > "$fixture/bin/cargo" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ $1 == metadata ]]; then
  printf '%s\n' '{"packages":[{"name":"titan_test"},{"name":"bevy_ecs"},{"name":"titan_doom"}]}'
else
  printf '%s|%s\n' "${RUSTDOCFLAGS:-}" "$*" >> "$COMMAND_LOG"
fi
SH
chmod +x "$fixture/bin/cargo"
packages='-p titan_doom -p titan_test'
assert_commands() {
  local mode=$1 expected=$2
  : > "$COMMAND_LOG"
  bash "$script_dir/titan-scoped-ci.sh" "$mode"
  if [[ $(< "$COMMAND_LOG") != "$expected" ]]; then
    printf 'Unexpected %s commands:\n%s\n' "$mode" "$(< "$COMMAND_LOG")" >&2
    exit 1
  fi
}
assert_commands lints "|fmt --all -- --check"$'\n'"|clippy $packages --all-targets --all-features --features bevy_remote/bevy_render -- -D warnings"
assert_commands test "|test $packages --lib --bins --tests --features bevy_ecs/track_location,bevy_remote/bevy_render"
assert_commands doc "|test $packages --doc --features bevy_remote/bevy_render"$'\n'"-D warnings|doc $packages --all-features --features bevy_remote/bevy_render --no-deps --document-private-items --keep-going"
if bash "$script_dir/titan-scoped-ci.sh" invalid >/dev/null 2>&1; then
  echo 'Invalid mode unexpectedly succeeded' >&2
  exit 1
fi
printf '#!/usr/bin/env bash\nprintf '\''{"packages":[]}'\''\n' > "$fixture/bin/cargo"
if bash "$script_dir/titan-scoped-ci.sh" test >/dev/null 2>&1; then
  echo 'Missing Titan packages unexpectedly succeeded' >&2
  exit 1
fi
echo 'Scoped Titan CI command tests passed'
