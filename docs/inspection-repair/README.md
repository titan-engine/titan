# Inspection failure repair evidence

[Issue #39](https://github.com/titan-engine/titan/issues/39) investigated three
failure categories: uncontrolled clock, denied mutation/control, and ambiguous
native runtime selection. Current output identifies each cause. Existing reads
are sufficient to discover the RPG's clock recovery, but the error does not name
those reads. Permission output does not expose the host-specific opt-in route;
ambiguity output does not name the CLI discovery command or selector.

This is evidence and a bounded recommendation, not implemented repair guidance.
No engine, protocol, permissions, or automatic repair behavior changes here.

## Reproduce

Run from the repository root on macOS or Linux:

```sh
cargo build --locked -p titan-cli -p titan --example procedural_rpg --bin titan
python3 docs/inspection-repair/native.py
python3 scripts/build-browser.py
node docs/inspection-repair/browser.mjs
```

The native probe starts two owned, bounded RPG processes in a temporary project,
uses the default diagnostic policy, reads selected bundle fields and `api.txt`,
and terminates its processes and checks registration cleanup. It never reads
raw discovery registrations. It edits no fields successfully; it resumes,
pauses, and steps its own fixture. Python assertions must remain enabled.
The browser probe runs actual compiled WASM under Node, including an explicitly
controlled fixture. It does not drive a DOM, message bridge, window, or GPU.
Neither probe is a new CI gate or a general repair API.

Recorded on 2026-09-05, macOS arm64, against engine revision
`5736101cbc6462b3bb51ce7617f8300605175b9a` (see the PR for the evidence commit).
Tool versions: Rust 1.98.1, Node 26.8.1, Python 3.9.6.
The scripts and JSON are an investigation snapshot; if messages change, examine
the new output rather than treating this snapshot as a compatibility oracle.

- [Native output](native-output.json): response envelopes, selected diagnostic
  bundle fields, component metadata, discovery and follow-up reads. Only project
  paths, request IDs, process IDs, endpoints and diagnostic paths are normalized.
- [Browser output](browser-output.json): exact request/response pairs from WASM.

## 1. Uncontrolled clock

Native setup starts paused, then invokes the RPG's registered `resume` command.
`step 1` returns this error (the native envelope additionally contains the
instance, frame 0, revision 1 and request/schema identity):

```json
{"code":"not_controlled","message":"the runtime clock is not controlled by the inspector","details":{"diagnostic_bundle":"<bundle.json>"},"retryable":false}
```

The actual-WASM live fixture explicitly enables controls, resumes, and submits
`{"type":"step","frames":1}`. It returns the same code, message and retryability,
with no `details` field, at frame 0/revision 2. These counters remain unchanged by
the rejection. The native headless loop does not tick automatically after resume;
this exercises clock ownership, not an interactive timing measurement.

The native bundle's `api.txt` includes `command pause`. Follow-up `status` says
`paused: false`; `capabilities` says `controlled: false`, omits `step`, and includes
`invoke`. `commands` on both runtimes lists `pause` with no arguments. With
control already authorized for these fixtures, invoking that discovered command
then stepping succeeds: native frame 1/revision 3 and browser frame 1/revision 4.

**Discoverability:** the failure alone names the cause, not a next read. The
native bundle provides a candidate command; the follow-up output on both
runtimes confirms a valid route without implementation source. A generic engine
must not assume every game registers `pause`. Safe guidance should point to
`status`, `capabilities`, and `commands`; invoke a host command only when it is
advertised and the action is authorized.

## 2. Denied mutation/control

The native probe discovers the player ID and qualified Position component key,
then requests `set-field 0 0 procedural_rpg::game::Position x --value 3` on the
RPG started without `--allow-mutation`:

```json
{"code":"mutation_disabled","message":"runtime mutation was not explicitly enabled","details":{"diagnostic_bundle":"<bundle.json>"},"retryable":false}
```

Entity reads before and after match: Position remains `(2, 2)`, frame/revision
remain zero. Metadata and `api.txt` mark `x` writable, while `capabilities` reports
`mutation_enabled: false` and omits `mutate`. Field writability does not grant
runtime permission. Native command invocation remains separately available.

The actual-WASM synchronous inspector constructed without control opt-in rejects
`step` at frame/revision zero:

```json
{"code":"mutation_disabled","message":"controls were not explicitly enabled","retryable":false}
```

Its capabilities say `controlled: true` and `mutation_enabled: false`, with only
`inspect`, `query`, and `capture`; `commands` is empty. This is a permission gate
even though the clock is paused. A separate, explicitly opted-in fixture accepts
stepping to frame 1/revision 1. The script does not automatically opt in the
rejected session.

**Discoverability:** output identifies the policy restriction and a capabilities
read distinguishes it from clock ownership. Neither bundle nor follow-up output
explains the host's opt-in route. The correct next step is to retain read-only
inspection and consult the selected host's local instructions. If a field edit
is authorized, the [native RPG instructions](../cli.md#native-headless-control)
require launch-time `--allow-mutation`; a CLI request cannot enable it on the
existing process. Restart/replacement needs authorization and may lose state.
The [browser instructions](../browser.md) explain explicit
page opt-in; the synchronous inspector starts a fresh game. Live hosts can have
different session behavior. These routes come from local documentation, not the
failure text, and must not be guessed from `mutation_disabled` alone. DOM opt-in
and a native relaunch with mutation enabled were not exercised by these probes.

## 3. Ambiguous native target

With `repair-a` and `repair-b` registered in one temporary project, CLI `status`
without `--instance` exits nonzero and emits a local failure rather than a
runtime response envelope:

```json
{"status":"failure","error":{"code":"ambiguous_target","message":"multiple runtime instances match; choose an instance explicitly","retryable":false,"details":{"diagnostic_bundle":"<bundle.json>"}}}
```

The fallback bundle repeats the local error; it has no runtime response or
`api.txt`. `instances` returns the two public IDs without tokens. Repeating
`status` with `--instance repair-a` succeeds. Selection here is intentional
because the probe owns both fixtures; an agent must not silently choose the
first result when the user's intended target is unknown.

**Discoverability:** the failure names the required decision but omits both the
next read (`instances`) and selector spelling (`--instance`). CLI help and the
[local CLI guide](../cli.md#native-headless-control) supply them. No browser
ambiguity equivalent was exercised: the in-process adapter already selects its
runtime. Missing targets and unknown errors were outside this sample.

## Output-only assessment

A separate agent assessed only the two JSON transcripts, without repository
source or documentation. It independently identified the clock recovery once
command metadata was supplied, the missing permission-enable procedure, and the
missing ambiguity enumeration/selector guidance. It could not justify choosing
one fixture from its identity alone. The final native transcript also records
the intermediate pause response and paused status, which the initial assessment
noted were omitted.

This is a qualitative navigation assessment, not a measured unaided-agent success
rate: the transcripts already include investigator-selected follow-up reads.
Sanitized process/endpoint fields also omit potentially useful target distinctions.
No claim is made that all agents discover these reads from the failure alone.

## Bounded recommendation

A small optional hint payload in the existing `error.details` map is warranted
for these demonstrated navigation gaps. The candidate fields below are a design
sketch for separate maintainer approval, not a new protocol contract:

| Observed case | Candidate guidance | Producing layer |
| --- | --- | --- |
| `not_controlled` | `next_reads: [status, capabilities, commands]`; discover an advertised clock-control action | Shared inspector |
| `mutation_disabled` | Read capabilities and consult host opt-in instructions; keep reads available | Policy owner, with a host-specific local documentation reference only when known |
| CLI `ambiguous_target` | `next_reads: [instances]`, selector `--instance`; explicitly select the intended runtime | CLI discovery adapter |

Hints should be bounded descriptive metadata, not executable repair commands.
Keep existing codes, messages, retryability, identity, frame/revision semantics,
and diagnostic paths. Native field policy and browser control policy share an
error code but must keep their distinct explanations. Do not infer a universal
permission-changing command, restart a game, or invoke `pause` without discovery
and authorization. Unknown errors get no invented repair.

The current protocol already accepts optional JSON-valued details. A later
implementation should verify omission and unknown-key tolerance with existing
clients, preservation of `diagnostic_bundle`, and equivalent shared hints across
native and WASM while retaining host-specific differences. Use these cases to
verify: unchanged rejection counters and fields; pause discovered before a
successful controlled step; read-only browser controls still denied; native
ambiguity still requires deliberate target selection. No protocol rewrite or
new exhaustive error taxonomy is justified by this sample.
