# Diagnostic helpers

This crate provides diagnostic data, image comparisons, and native safe-point
collection. `DiagnosticInspector` wraps `Inspector::handle`, records recent
requests, and writes bundles on failures by default. The native RPG serve loop
uses it automatically; the CLI also bundles local, transport, and Cargo failures.
See [CLI diagnostics](../../docs/cli.md) for policies and execution budgets.

The wrapper snapshots up to 1,000 entity IDs, names, and component lists, records
request and collection timings, and emits component/command API metadata. A
read-only host callback adds game-specific values and an optional image. The RPG
includes quest state, positions, and a software PNG. Failed capture attempts are
logged; failed persistence is returned separately without replacing the original
protocol outcome. Browser and direct Rust test hosts can use the portable helpers,
but automatic filesystem collection is currently a native integration.

`DiagnosticPolicy` defaults to `OnFailure` and also supports `Always` and `Never`.
`RequestHistory` retains an oldest-first sequence of typed request/response pairs
bounded by both entry count and serialized entry bytes (default 64 entries and
1 MiB). Snapshots distinguish accepted scheduled input from rejected requests and
report dropped entries. Sequence and input ordering are deterministic; elapsed
microseconds are supplied by the host. This is recent diagnostic context, not a
complete replay log.

`DiagnosticBundle::new(request, response)` captures runtime envelopes without
changing their frame/revision fields. The host supplies relevant world state,
history, logs, timings, context, and optional API metadata. For CLI-local errors,
`DiagnosticBundle::local_failure(error_json)` leaves request/response absent and
never fabricates runtime facts. `ApiComponent::from_metadata` accepts base Titan
component metadata; callers can enrich its reflected fields. `ApiSummary` emits
a compact sorted summary including command argument types and constraints.

Native `write_bundle(root, &bundle, Option<&Image>)` creates a unique directory
containing `bundle.json`, optional RGBA PNG capture, and optional `api.txt`.
Request IDs never enter filesystem paths. Each new Unix directory/file is
owner-only. Capture and API files finish before the manifest is atomically
renamed into place; failed writes clean up the new directory. Each manifest and
raw capture is limited to 64 MiB. Existing capture descriptors are replaced by
the supplied image or cleared when no image is supplied, so the manifest does
not accidentally point outside the bundle. The writer returns absolute paths.
After a successful write, `attach_failure_path` may add the manifest path to
`error.details.diagnostic_bundle`. It leaves successful responses unchanged;
hosts can return or log their `WrittenBundle` separately in Always mode.

`compare_images` reports exact RGBA equality, changed pixels, maximum/mean channel
error, structural similarity, and linear RGB RMSE. Exact mode requires equal
bytes. Default perceptual mode requires SSIM ≥ 0.99 and RMSE ≤ 0.01; callers can
configure thresholds and an optional maximum byte error. Dimensions must match.
Both images are composited over black and white in linear sRGB so transparent
appearance and alpha changes are evaluated. Invisible RGB differences remain
visible in the exact metrics. SSIM measures luminance structure, while RGB RMSE
also catches color changes. Empty matching images are exactly equal.

The SSIM implementation uses the normalized formula from
[Wang et al., 2004](https://www.cns.nyu.edu/pub/lcv/wang03-reprint.pdf), with
K1 = 0.01, K2 = 0.03, and population moments. It deliberately uses non-overlapping
8×8 blocks weighted by pixel count, rather than the original Gaussian window;
it is a block SSIM variant, not a drop-in reproduction of that implementation.
Linearization follows the sRGB transfer function in
[W3C CSS Color 4](https://www.w3.org/TR/css-color-4/#color-conversion-code).
Perceptual thresholds are engineering tolerances, not a universal judgment of
visual equivalence; exact software captures remain the deterministic reference.

## Offline comparison reports

On native targets, `write_comparison_report(root, expected, actual, options)`
creates a unique owner-only directory below `root`. It writes lossless copies as
`expected.png` and `actual.png`, a spatial `difference.png`, and `report.json`.
The JSON records the supplied `ComparisonOptions`, the existing `ImageComparison`
metrics without changing their calculation, artifact names, dimensions, and the
difference encoding. A failed write removes its newly created directory; an
existing report is never overwritten.

The opaque RGBA difference image uses independent channels so exact differences
remain locatable even when they are not visible after compositing:

- Red is the largest linear-RGB appearance error after compositing both pixels
  over black and white, scaled with `ceil(error * 255)`. Any visible difference
  therefore has nonzero red; brighter red means a larger visible error.
- Green is the absolute alpha-byte error. Alpha-only changes appear green plus
  any red contributed by their visible effect.
- Blue is the largest raw RGB-byte error only when the composited visible error
  is zero. Thus RGB changes hidden by zero alpha remain blue instead of vanishing.
- Alpha is always 255. Identical pixels are opaque black. Channels can combine;
  for example, a fully visible alpha change can appear yellow.

Images must have equal dimensions, matching `compare_images`. Invalid thresholds
and dimension mismatches are returned before output is created. Empty equal images
remain valid for `compare_images`, but reports reject them because PNG cannot
encode zero dimensions. Each input's decoded RGBA bytes and each encoded report
artifact are limited to 64 MiB. Filesystem, JSON, PNG, empty-image, and size errors
have distinct `ComparisonReportError` variants.

Images already in memory can be passed directly. For images on disk, read their
bytes and decode each with `Image::from_png` and bounded `ImageDecodeLimits`, then
pass the decoded images to the report helper. A deliberately changed fixture can
be generated and inspected with:

```sh
cargo run -p titan-diagnostics --example comparison_report -- target/visual-diffs
```

The command prints the created report directory. Inspect its three PNG files and
use `report.json` for automation. Report generation is offline and does not alter
capture transport, baselines, thresholds, or reference checksums.

Run `cargo test -p titan-diagnostics`. Portable data/history/comparison APIs also
compile for `wasm32-unknown-unknown`; filesystem helpers are native-only.
