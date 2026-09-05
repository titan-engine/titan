# Two file-backed RPG sprites

The RPG loads `player.png` and `tree.png` at startup in native, browser and headless
hosts. The committed 8×10 player and 18×18 tree images exactly export the existing
procedural sprites;
the completed route still renders `f7a298f62ad75c1c`. Both sources become the same
engine `Image` and use the existing image store, extraction and renderers.

## Iterate without rebuilding

From the repository root:

```sh
cargo run --example play_rpg
cargo run --example procedural_rpg -- --assets-dir /absolute/path/to/assets
cargo run --example replay_rpg -- docs/replay/rpg-recording.json --assets-dir /absolute/path/to/assets
python3 scripts/build-browser.py
python3 -m http.server --directory web 8080
python3 scripts/build-rpg-app.py
```

Native runners accept `--assets-dir DIR`; otherwise they load both PNGs in `assets/`
relative to the working directory. A macOS bundle instead uses its adjacent
`Contents/Resources/assets/` directory, even after relocation. Missing bundle
resources fail explicitly rather than falling back to the working directory.
The app builder prints the absolute `.app` path. These are unsigned local bundles.

Replace either native file and restart the process to see new pixels with the same
binary. Browser builds copy source `assets/` to `web/assets/`; edit that served
copy and reload `/play/` or `/inspector/` without rebuilding WASM. The next build
replaces the served copy from source. Keep lasting changes in root `assets/`.
Native bundle resources can likewise be replaced directly during iteration.

Native `--generated-assets` explicitly selects the procedural comparison and
cannot be combined with `--assets-dir`. Regenerate the committed fixtures with:

```sh
cargo run --example procedural_rpg -- --export-player-png assets/player.png
cargo run --example procedural_rpg -- --export-tree-png assets/tree.png
```

Startup must finish loading and decoding both images before a playable world is
installed. A successful first image never creates a partial game if the second
fails. Diagnostics name the failed file; repairing it and retrying loads a fresh
pair. The two reads are not an atomic filesystem snapshot: replace files before
starting the process or reloading the page.
Native failures identify the path and repair action. Browser fetch/decode failures
show a retry action; failed inspector resets clear prior world controls and
captures. Fetches bypass the browser cache, time out after ten seconds, and enforce
the encoded limit while streaming. Retrying after repairing the file starts a
fresh session.

## Engine and host boundaries

The default-enabled `image-png` Cargo feature exposes `Image::from_png(bytes,
ImageDecodeLimits)`. Disable default features for a procedural-only core; the
three PNG-dependent RPG examples declare their required feature. The decoder uses
`png` and converts supported static PNG color types, palettes and bit depths to
straight RGBA8 without gamma conversion. Animated PNGs, malformed/truncated data
and invalid checksums are rejected.

Engine defaults allow 8 MiB encoded data, dimensions up to 4096×4096 and 64 MiB
of decoded pixels. Hosts can narrow those limits. The RPG permits 256 KiB encoded,
64×64 pixels, 16 KiB decoded output and a 2 MiB decoder allocation budget. The
allocation budget uses the decoder's best-effort accounting; it is not a process
memory ceiling. Dimensions and engine output sizes are checked before allocation.
Native loading checks file type/size and bounds the read. Filesystem lookup and
browser readiness stay host-owned; the decoder accepts bytes without platform I/O.

The startup pair is retained through in-game restart, save/load and interactive
playback. Those operations do not reread either source. Snapshots contain game
state, not image bytes or paths. New recordings carry each role's dimensions and
RGBA checksum in their final state, as well as the final rendered checksum.
Fresh replay verification loads both supplied images and checks their identities
and final pixels. These checksums detect mismatches; they are not cryptographic
content identifiers. Legacy recordings without pair identity still verify their
final pixels. Hosts use the pair-aware verifier; legacy procedural and player-only
constructors/verifiers remain comparison helpers (the latter generate the tree).

## Identity, ownership and dependencies

`RpgImages { player, tree }` is a game-local pair of owned decoded `Image` values.
The role identifies usage; a filename identifies the host's source, and neither
is a universal engine identifier. Native hosts resolve a directory; browser hosts
fetch two URLs. The game retains both decoded values as a world resource and
copies them into its existing `ImageAssets` store when constructing render art.
Store-local `ImageId` values are runtime handles and are not serialized source
identity. All tree entities share the tree handle; replacing `tree.png` changes
all trees while preserving the player. Dropping the world releases its images.

The playable world depends on both images being ready. The two sprites have no
dependency on each other, and their decoded pixels use the same renderer boundary
as generated images. This exercise needs explicit roles and an all-or-nothing
startup boundary, but supplies no evidence requiring a shared asset manager or
dependency graph. File changes affect the next startup, never a live session.

Shared build tooling stages regular resource files into browser or macOS output
and rejects symlinks and overlapping source/output trees. RPG wrappers require
the source directory. The starter and arena still work without an asset directory.
See [build helper conventions](host-tooling.md).

This completes a bounded two-image exercise. It does not implement hot
reload, general asset identity/dependency tracking, generation caches, a general async
asset server, other external formats, or the eventual native format. The broader
[asset requirements](vision.md#rendering-and-assets) remain scheduled separately.

## Verification

```sh
python3 scripts/test-rpg-assets.py --gpu # desktop/macOS bundle path
python3 scripts/build-browser.py
node scripts/test-rpg-assets.mjs
node --test web/inspector/*.test.mjs web/shared/*.test.mjs web/play/*.test.mjs
TITAN_GPU_TOLERANCE=0 cargo test -p titan-render-wgpu --test offscreen completed_rpg_replay -- --ignored
```

Without `--gpu`, the Python acceptance is portable headless verification. It
covers unchanged reference pixels, independent player/tree replacements with the same binary,
path/size/decode diagnostics and image-aware fresh replay. The GPU path also
runs relocated bundles with replacement/missing/explicit-override resources.
Actual WASM checks both runtimes, invalid images, retained startup pairs,
recording round trips and native cross-verification. The GPU comparison decodes
both committed PNGs and checks open/closed journal views against software.
Physical native and browser canvas inspection confirms the normal sprite;
reloading the browser after replacing only its served PNG visibly changes the
sprite, and repairing invalid bytes makes Retry succeed. [Earlier single-image evidence](assets/checks.json) records the original regression
suite; issue #8 and its PR record the two-image verification. CI includes the new native/browser/bundle checks.
