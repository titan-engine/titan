# Verification evidence lifecycle

Evidence retention depends on its purpose and active consumers, not its extension
or size. JSON may be a required replay input; a small screenshot may be incidental.
This policy governs future evidence and review of existing material. It does not
remove files, relocate runners, relax verification gates or change references.

## What belongs in source control

Keep intentional regression inputs and golden assets consumed by tests, current
explanatory illustrations, reproducible methodology, and compact meaningful
baseline summaries. Identify the test, guide or maintained claim that consumes
each retained item. Preserve the accepted golden checksums, replay inputs required
by tests, and the committed crisp README preview. Changing a reference requires
approved intentional behavior/visual scope and reviewed before/after verification.

Remove incidental run output from HEAD when it has no active consumer: full logs,
repeated state snapshots, failed attempts and incidental screenshots belong in
run bundles by default. Review consumers and links before removal; migrate a
needed fixture or illustration explicitly instead of deleting it by file type.
Do not dump chronological task logs into docs or move the same clutter into
`docs/archive`. Keep durable conclusions and methodology in focused guides.

A retained historical summary must name the full source SHA (and any source
patch/dirty state), meaningful outcome, limitations, and a reproduction pointer
with commands, inputs and relevant environment. Label revision-specific
observations as historical; they do not establish current behavior or replace
current verification instructions. Name the evidence location supporting a
lasting claim, including its revision/path or attachment link.

## Run bundles and retention

Write maintained reruns to ignored output such as root `target/evidence/<run>/`,
or a temporary directory outside the checkout. Check that an alternative local
output path is ignored. Keep capture identity JSON beside its image, and preserve
commands, failures and corrections in the sanitized run bundle for review.
Promote only selected fixtures, illustrations or compact summaries into source
through an explicit reviewed change; a rerun must not silently replace a baseline.
Legacy historical scripts that write beside themselves must run in a disposable
checkout, with results copied to ignored output in the maintained checkout.
Runner relocation and bulk cleanup are separate work.

Use existing GitHub Actions artifacts for transient troubleshooting bundles.
Record the run URL, full source SHA, job/attempt, outcome and limitations in the
PR or issue; state the artifact expiry. Local ignored bundles remain until the
owner deletes them and are not shared evidence. Expiring Actions artifacts alone
cannot support lasting claims: before expiry, attach the necessary sanitized
bundle to the relevant GitHub issue/PR and verify its download is accessible to
repository reviewers. These attachments are the durable location, independent of
Actions retention; keep their links in the compact summary and do not delete them
while a maintained claim relies on them. If attachment limits prevent preserving
the needed material, narrow the claim to durably available evidence and document
the limitation. Do not introduce paid storage or create releases/tags for evidence.

Existing committed evidence is already accessible at immutable Git revisions.
When removing it from HEAD, link to its full commit SHA and repository path
(e.g. a GitHub `blob/<full-SHA>/<path>` permalink); it need not all be reuploaded.
Distinguish the evidence-containing revision from the measured source revision.
Sanitize bundles before sharing: omit credentials, discovery registrations,
private session identifiers and unrelated machine data; inspect GUI captures too.

## Applying the policy

| Case | Retention decision and verification |
| --- | --- |
| Golden/replay fixture | Keep `games/arena/tests/fixtures/recording-v1.json`: the native control test and live replay tests consume it. Preserve RPG/arena checksum assertions and the README preview. Generated appearance does not make a fixture disposable. |
| Benchmark baseline | Keep compact workload, environment, timing boundaries, result and limitation summaries such as `docs/sparse-churn.md`, with source SHA and reproduction pointer. Full repeated snapshots go in a bundle; retain machine-readable inputs/results in source only when an identified consumer needs them. |
| Failed run | Keep the bounded package below locally or in the existing expiring CI artifact while diagnosing it. Record the failure and resolution in the issue/PR; preserve a durable sanitized bundle only if a lasting claim relies on its contents. A regression input extracted from the failure is promoted separately. |
| Manual GUI capture | Inspect the actual native/browser image and retain its identity/backend in the run bundle. Promote an image only when a current guide needs it or attach it durably if a lasting visual claim depends on it. Headless/Node success still cannot substitute for GUI verification. |
| Historical experiment | For `docs/evidence/agent-iteration/`, retain useful methodology and a compact revision-specific outcome/limitations summary. Existing raw observations can be referenced at an immutable revision when removed from HEAD; future full reruns use ignored output. This policy does not reclassify skeleton observations as finished-game acceptance. |

All required native/browser/headless checks, image inspection, failure reporting
and owned-process cleanup still apply. Retention is a decision about where their
outputs live, not whether to perform or report verification.

## Native acceptance failure evidence

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

Only these files can be retained and uploaded by this failure collector:

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

In GitHub Actions, the native and macOS workload shards run these core
acceptance checks; the three required check names are aggregate gates. On failure, the workflow uploads only the seven
explicit paths above. Find the download in the workflow run's Artifacts section:
`acceptance-failures-<job>-<workload>-<OS>-<run-id>-<attempt>`. Retention is seven days. A job
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
