# Sunlit meadow: recorded agent iteration

The user selected **A, Sunlit meadow** from three procedural concept sketches
(meadow, moonlit ruins, and four-color retro). This applies that direction to the
example RPG; it does not impose an art style on the engine.

Open [the comparison](index.html) in a browser to switch between startup and
completed-quest captures. All images are real software-rendered game frames,
not concept sketches. The adjacent `evidence.json` files record the source
revision, dimensions, checksums, and protocol-driven semantic assertions.

## Change and visual review

The original dense grass texture and isolated sprites become a quieter seeded
meadow, warm path following the actual shard route, leafy border trees, scattered
flowers and rocks, a red-clothed player, cyan shards, and a mossy shrine. The
shrine rises behind its interaction tile so the player and activated monument
remain distinguishable at the end of the route. Border foliage was pulled into
view after reviewing the first capture.

Agent visual review checked startup and completion: the route is visible,
collectibles contrast with the ground, scenery frames the play area, and the
player remains readable at the shrine. The user approved both the meadow direction and the completed demo.

Scenery is render-only and does not imply collision. Movement remains legal
across the full map. The existing fixed whole-map view is sufficient for this
small slice; no scrolling camera or new gameplay was added.

## Evidence

| Capture | Original | Sunlit meadow |
| --- | --- | --- |
| Startup, frame 0 | `65576d60e54754b8` | `04bc76cc9297adaf` |
| Completed, frame 11 | `98618cd721c5b52d` | `190a92085def5677` |
| Dimensions | 160 × 112 | 160 × 112 |
| Completed state | 3 collected shards, active shrine | 3 collected shards, active shrine |

Both recordings inject the same right-2, down-3, right-6 input sequence through
separate CLI processes. The resulting two entities are the player and active
shrine. Existing Rust semantic assertions independently verify the three pickups.

Validation passed: formatting, workspace tests, strict Clippy, WASM target check,
native separate-process control loop (including field edits and diagnostics),
actual release WASM control loop, and browser bridge tests. The opt-in native
GPU RPG readback matched `190a92085def5677` exactly on both `Rgba8Unorm` and
`Rgba8UnormSrgb`, with `TITAN_GPU_TOLERANCE=0`. The real browser GPU
player was also launched and its reference replay visibly completed at frame
11 with three shards and an active shrine.

## Reproduce

Build the current game and CLI, then choose a new output directory:

```sh
cargo build -p titan-cli -p titan --bin titan --example procedural_rpg
python3 scripts/capture-rpg-evidence.py target/titan/new-art-evidence
```

The capture script launches a bounded paused runtime, requests startup and
replay captures, converts RGB PPM losslessly to PNG, records assertions, and
shuts down the runtime. It does not rebuild: build the revision being evaluated
first. Historical baseline images stay unchanged when the reference changes.

For the GPU comparison:

```sh
TITAN_GPU_TOLERANCE=0 cargo test -p titan-render-wgpu --test offscreen completed_rpg_replay -- --ignored --nocapture
```
