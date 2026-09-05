# Native acceptance failure evidence

The RPG `scripts/test-control-loop.py` and arena
`games/arena/scripts/test-control.py` retain evidence when an assertion, command,
or startup fails. They snapshot the latest returned diagnostic bundle and capture
while their temporary project still exists, then clean up the owned runtime as
usual. The collector never turns a failing test into a successful exit; a failure
to collect evidence is reported separately.

A failed run prints its package directory under
`target/acceptance-failures/<test>-<unique-run>/` at the repository root, regardless
of Cargo's target directory. Set `TITAN_ACCEPTANCE_EVIDENCE_DIR` to choose another
local destination. Passing runs discard the collector's in-memory snapshots and
write no package. Existing game-specific reference/replay evidence is separate;
it is never included in this upload. Local failed packages remain until deleted.

Only these files can be retained and uploaded:

| File | Contents |
| --- | --- |
| `context.json` | Test/run identity, exception and assertion traceback, owned runtime PIDs, collection limits/errors |
| `commands.log` | Recent command arguments, return codes, stdout/stderr |
| `runtime.log` | Bounded combined build/runtime stdout and stderr |
| `bundle.json` | Latest available diagnostic manifest, sanitized |
| `api.txt` | API context associated with that diagnostic |
| `capture.png` | Diagnostic capture, when present and valid |
| `latest-capture.ppm` | Latest explicit software capture returned by the CLI |

Text streams keep at most 128 KiB, diagnostic JSON at most 512 KiB, images at
most 2 MiB each, and a package at most 6 MiB. Runtime output is continuously
drained into bounded memory rather than an unbounded temporary file. Large,
invalid, symlinked or unexpected artifacts are omitted; the context reports
collection errors. Only the latest bundle/capture is retained, so a later
successful operation may make the retained capture newer than the diagnostic.

The collector excludes discovery registrations and unrelated target contents.
It removes credential-shaped JSON fields, bearer credentials and common secret
assignments from text; the harness also registers any authentication token it
reads for redaction. Image ingestion validates the native image structure and
rejects metadata/trailing payloads. This is a bounded collector for these known
acceptance fixtures, not a general-purpose sanitizer for arbitrary application
secrets or screenshots.

In GitHub Actions, the Native checks and macOS development app bundles jobs run
these core acceptance checks. On failure, the workflow uploads only the seven
explicit paths above. Find the download in the workflow run's Artifacts section:
`acceptance-failures-<job>-<OS>-<run-id>-<attempt>`. Retention is seven days. A job
failure before either harness collects evidence has no package to upload.

Run the collector's security/limit regressions and the real controlled-failure
verification with:

```sh
python3 scripts/test-acceptance-evidence.py
python3 scripts/test-acceptance-failure-integration.py
```

The integration check injects an assertion after each real runtime has returned
a diagnostic and capture. It checks the original nonzero status, exact package
allowlist, size bound, stopped child PIDs and removed arena registrations, then
deletes its verification packages. Normal core acceptance runs verify the passing
path. For a package to inspect manually:

```sh
TITAN_ACCEPTANCE_FAIL=rpg-control:diagnostic python3 scripts/test-control-loop.py
TITAN_ACCEPTANCE_FAIL=arena-control:diagnostic python3 games/arena/scripts/test-control.py
```

These commands intentionally exit unsuccessfully. Unset the variable for normal
runs. No browser artifacts, discovery files, or full target-directory uploads are
part of this mechanism.
