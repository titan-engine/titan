# Bounded PNG generated-input coverage

`Image::from_png` has a deterministic seeded mutation harness in
`examples/png_fuzz.rs`. It exercises the existing decoder without changing its
supported formats, limits, or reference images. The required Native checks job
runs the retained corpus and 1,000 generated inputs with seed 69.

```sh
python3 scripts/fuzz-png.py --seed 69 --iterations 1000
python3 scripts/fuzz-png.py --seed 12345 --iterations 10000
python3 scripts/test-png-fuzz-runner.py
```

Each command is a bounded campaign. Choose a different decimal u64 seed for
additional reproducible exploration; iterations are capped at 1,000,000 and
remain subject to the same process deadline. Compilation uses the existing
acceptance build deadline (1,200 seconds by default). Cargo's target directory
is discovered through metadata, including when `CARGO_TARGET_DIR` is set.

The harness checks 15 valid color/depth seeds (including palette transparency
and 16-bit reduction), every truncated prefix, explicit decode-limit boundaries,
and eight limit combinations per generated input. Mutations include byte flips,
CRC-repaired IHDR/IDAT changes, and chunk insertion, deletion, duplication and
reordering. Successful decodes must have positive bounded dimensions and exactly
width × height × 4 RGBA bytes within the configured limits. Known-valid seeds
assert success, and known-invalid/boundary cases assert rejection.

The retained corpus is sorted by filename and capped at 64 JSON cases, each at
most 512 KiB on disk with at most 64 KiB of encoded input. Generated inputs share
the 64 KiB input bound; normal generated limits allow at most 1 MiB decoded RGBA
and 4 MiB API allocation budget. Replay preserves the original configured limits.
The initial six corpus cases cover valid RGBA, empty/signature-only input, missing
IEND, corrupt IHDR CRC and APNG rejection. No new decoder defect was found when
establishing these cases. Seed generation uses the locked PNG encoder dependency;
reproduction assumes the same source revision and lockfile.

## Process containment and failures

The Python wrapper starts the decoder in an owned process group with a
60-second wall deadline, a hard 30-second CPU limit, an 8 MiB per-file write
limit, and core dumps disabled. Linux CI additionally enforces a hard 1 GiB
virtual-address-space limit. macOS polls direct-child RSS every 20 ms and stops
the child above 512 MiB; this is a sampled guard that can overshoot, not a hard
memory ceiling. The decoder harness does not spawn children. These limits apply
to the decoder process, not Cargo. The API allocation budget remains a separate,
best-effort accounting boundary and does not limit all process memory.

Before decoding, the harness writes the complete current case (bytes, limits,
and label) to `current.json`. A failed run retains that file, `run.log`, and
`run.json` under `target/png-fuzz/run-*`. The wrapper reports nonzero exits,
signals, invariant failures, wall timeouts and sampled RSS violations as failures;
its owned-process helper terminates and reaps the group. Failure before the first
case may have no `current.json`; inspect the log and run configuration. CI uploads
this evidence for seven days. Successful run directories are removed. Local
failed runs remain until explicitly removed, so repeated failing campaigns can
accumulate evidence on disk.

```sh
python3 scripts/fuzz-png.py --replay target/png-fuzz/run-EXAMPLE/current.json
```

A replay still has all process guards. The direct Rust executable is intended
for harness development only; use the Python wrapper for bounded campaigns.

## Regression retention and limitations

Keep a failing case's limit configuration fixed while reducing its byte array,
replaying after each reduction to ensure the same failure remains. Reduce
irrelevant chunks first, then contiguous byte ranges; minimize limit fields only
when that preserves the failure. Add the smallest reproducer to
`fixtures/png-corpus` with an explanatory label and a deterministic expectation.
A newly discovered decoder defect that needs broader implementation changes
requires separately agreed scope. Existing malformed cases are regression seeds,
not evidence of newly discovered decoder bugs.

This is deterministic generated-input testing, not coverage-guided libFuzzer,
a sanitizer campaign, a formal proof, or an exhaustive PNG conformance suite.
Passing results establish only that the tested inputs satisfy the harness
invariants and do not panic within these process budgets. Native CI does not
establish equivalent WASM fuzz coverage. Interlaced images, rare compressed-stream states and large
images beyond the harness input/output bounds remain underexplored; exact
reference checksums and existing format tests remain independent gates.
