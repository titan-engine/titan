#!/usr/bin/env bash
# Verify scoped command construction without compiling the engine.
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin"
export COMMAND_LOG="$fixture/commands"
export REAL_JQ
REAL_JQ=$(command -v jq)
# Keep control bytes out of exported variables: native Windows processes can
# translate those bytes when passing the environment back to Git Bash.
export TOOL_ENDING=LF
export PATH="$fixture/bin:$PATH"
cat > "$fixture/bin/cargo" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ $1 == metadata ]]; then
  metadata='{"packages":[{"name":"titan_test"},{"name":"bevy_ecs"},{"name":"titan_doom"}]}'
  if [[ "$TOOL_ENDING" == CRLF ]]; then
    printf '%s\r\n' "$metadata"
  else
    printf '%s\n' "$metadata"
  fi
else
  for arg in "$@"; do
    if [[ "$arg" == *$'\r'* ]]; then
      printf 'Carriage return in Cargo argument: %q\n' "$arg" >&2
      exit 1
    fi
  done
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
assert_modes() {
  assert_commands lints "|fmt --all -- --check"$'\n'"|clippy $packages --all-targets --all-features --features bevy_remote/bevy_render -- -D warnings"
  assert_commands test "|test $packages --lib --bins --tests --features bevy_ecs/track_location,bevy_remote/bevy_render"
  assert_commands doc "|test $packages --doc --features bevy_remote/bevy_render"$'\n'"-D warnings|doc $packages --all-features --features bevy_remote/bevy_render --no-deps --document-private-items --keep-going"
}

# Exercise the installed jq too (native jq.exe on the Windows Actions runner).
assert_modes
printf 'Scoped Titan CI arguments passed with installed jq: %s\n' "$REAL_JQ"
# Force both cargo's JSON and jq's line-oriented output to use CRLF, even on
# Unix hosts. The wrapper still evaluates the production filter with real jq.
cat > "$fixture/bin/jq" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
"$REAL_JQ" "$@" | tr -d '\r' | while IFS= read -r name; do
  if [[ "$TOOL_ENDING" == CRLF ]]; then
    printf '%s\r\n' "$name"
  else
    printf '%s\n' "$name"
  fi
done
SH
chmod +x "$fixture/bin/jq"
for ending in LF CRLF; do
  TOOL_ENDING=$ending
  # Verify that the CRLF fixture really reaches the parser, not just its label.
  actual=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')
  expected=titan_test
  if [[ "$ending" == CRLF ]]; then
    expected+=$'\r'
  fi
  if [[ "$actual" != "$expected" ]]; then
    printf 'Bad %s jq fixture: expected %q, got %q (jq=%s)\n' "$ending" "$expected" "$actual" "$(command -v jq)" >&2
    exit 1
  fi
  assert_modes
  echo "Scoped Titan CI arguments passed with $ending tool output"
done
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
