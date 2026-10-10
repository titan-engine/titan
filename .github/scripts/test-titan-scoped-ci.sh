#!/usr/bin/env bash
# Verify scoped command construction without compiling the engine.
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin"
export COMMAND_LOG="$fixture/commands"
# Production recipes write summaries; fixtures must not pollute the caller's
# Actions summary with synthetic package lists or fallback cases.
export GITHUB_STEP_SUMMARY="$fixture/summary"
export REAL_JQ
REAL_JQ=$(command -v jq)
export TOOL_ENDING=LF
export PATH="$fixture/bin:$PATH"
cat > "$fixture/bin/cargo" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ $1 == metadata ]]; then
  # Node is native on Windows, like cargo/jq; MSYS shell producers can translate
  # CRLF in their pipes, defeating a fixture that only uses Bash printf.
  node -e '
    const dependencies = ["bevy_ecs", "bevy_remote"].map(name => ({name, kind: null, optional: false, uses_default_features: true}));
    const metadata = {packages: [{name: "titan_test", dependencies}, {name: "bevy_ecs", dependencies: []}, {name: "bevy_remote", dependencies: []}, {name: "titan_doom", dependencies}]};
    if (process.env.EXTRA_METADATA === "1") {
      metadata.packages.push(...["titan_remote", "titan_mcp", "titan_puzzle"].map(name => ({name, dependencies})));
    }
    process.stdout.write(JSON.stringify(metadata) + (process.env.TOOL_ENDING === "CRLF" ? "\r\n" : "\n"));
  '

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
  local mode=$1 expected=$2 selection=${3:-'*'}
  : > "$COMMAND_LOG"
  : > "$GITHUB_STEP_SUMMARY"
  bash "$script_dir/titan-scoped-ci.sh" "$mode" "$selection"
  if [[ -n $expected && ! -s $GITHUB_STEP_SUMMARY ]]; then
    echo 'Missing recipe summary' >&2
    exit 1
  fi
  if [[ $(< "$COMMAND_LOG") != "$expected" ]]; then
    printf 'Unexpected %s commands:\n%s\n' "$mode" "$(< "$COMMAND_LOG")" >&2
    exit 1
  fi
}
assert_modes() {
  assert_commands lints "|fmt --all -- --check"$'\n'"|clippy $packages --all-targets --all-features --features bevy_remote/bevy_render -- -D warnings"
  local test_command="|test $packages --lib --bins --tests --features bevy_ecs/track_location,bevy_remote/bevy_render"
  RUNNER_OS=Linux assert_commands test "$test_command"$'\n'"|test $packages --benches --features bevy_remote/bevy_render"
  RUNNER_OS=Windows assert_commands test "$test_command"
  RUNNER_OS=macOS assert_commands test "$test_command"
  assert_commands doc "|test $packages --doc --features bevy_remote/bevy_render"$'\n'"-D warnings|doc $packages --all-features --features bevy_remote/bevy_render --no-deps --document-private-items --keep-going"
  RUNNER_OS=Linux assert_commands test '|test -p titan_test --lib --bins --tests --features bevy_ecs/track_location,bevy_remote/bevy_render'$'\n''|test -p titan_test --benches --features bevy_remote/bevy_render' titan_test
  assert_commands extra '|test -p titan_test --all-features' titan_test
  assert_commands extra '|test -p titan_doom --no-default-features'$'\n''|clippy -p titan_doom --no-default-features --all-targets -- -D warnings'$'\n''|test -p titan_doom --all-features'$'\n''|test -p titan_test --all-features' '*'
  assert_commands extra '|test -p titan_doom --no-default-features'$'\n''|clippy -p titan_doom --no-default-features --all-targets -- -D warnings'$'\n''|test -p titan_doom --all-features' titan_doom
  EXTRA_METADATA=1 assert_commands extra '|test -p titan_mcp --all-features --features bevy_remote/bevy_render'$'\n''|test -p titan_remote --all-features --features bevy_remote/bevy_render' 'titan_remote titan_mcp'
  EXTRA_METADATA=1 assert_commands extra '|test -p titan_puzzle --no-default-features'$'\n''|clippy -p titan_puzzle --no-default-features --all-targets -- -D warnings'$'\n''|test -p titan_puzzle --all-features' titan_puzzle
  assert_commands test '' none
  assert_commands extra '' none
  # Stale/invalid outputs must run all packages, not silently drop tests.
  RUNNER_OS=Linux assert_commands test "$test_command"$'\n'"|test $packages --benches --features bevy_remote/bevy_render" titan_missing
}

# Exercise the installed jq too (native jq.exe on the Windows Actions runner).
assert_modes
printf 'Scoped Titan CI arguments passed with installed jq: %s\n' "$REAL_JQ"
# Force both cargo's JSON and jq's line-oriented output to use CRLF, even on
# Unix hosts. The wrapper still evaluates the production filter with real jq.
cat > "$fixture/bin/jq" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
"$REAL_JQ" "$@" | node -e '
  let text = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", chunk => { text += chunk; });
  process.stdin.on("end", () => {
    const eol = process.env.TOOL_ENDING === "CRLF" ? "\r\n" : "\n";
    process.stdout.write(text.replace(/\r/g, "").replace(/\n/g, eol));
  });
'
SH
chmod +x "$fixture/bin/jq"
for ending in LF CRLF; do
  TOOL_ENDING=$ending
  # Verify raw bytes through a file, not command substitution: MSYS can translate
  # CRLF at shell/pipe boundaries. The fixture must really emit titan_test + EOL.
  cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name' > "$fixture/jq-output"
  actual=$(od -An -tx1 "$fixture/jq-output" | tr -d '[:space:]')
  expected=746974616e5f746573740a
  if [[ "$ending" == CRLF ]]; then
    expected=746974616e5f746573740d0a
  fi
  if [[ "$actual" != "$expected" ]]; then
    printf 'Bad %s jq fixture bytes: expected %s, got %s (jq=%s)\n' "$ending" "$expected" "$actual" "$(command -v jq)" >&2
    node -e 'console.error("Native producer TOOL_ENDING:", process.env.TOOL_ENDING)'
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
