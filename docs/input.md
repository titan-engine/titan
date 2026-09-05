# Interactive input boundaries

`BufferedButtons<A>` accumulates held buttons and optional presses between fixed
ticks. Games choose which actions buffer, when to consume presses, and how to
sample them into `InputFrame`. `set` merges repeated logical presses; physical
aliases must be combined first. `take_presses` coalesces taps until consumed.
`cancel` discards one action and its pending press; `clear` discards all.

RPG keeps its six-tick movement cadence; arena buffers dash edges and retains
its direction/cooldown rules; starter retains continuous movement. No game
advancement or key bindings moved into the engine.

The shared `web/shared/input.mjs` controller owns browser keyboard/pointer
sources and focus lifecycle. Ordinary release preserves a short tap. Blur,
pause and visibility loss explicitly cancel buffered input. A canceled pointer
cancels its action only when no other source holds that action, preserving
unrelated queued taps. Hosts provide `setAction`, `cancelAction`, `clearInput`,
bindings and their local restart/replay behavior.

Browser build tooling copies the module from the Cargo-resolved Titan dependency
to standalone games; generated copies are ignored. Run the documented browser
build before serving a copied game. There is no RPG asset dependency.

## Verified regression

Before the fix, an RPG up/right tap followed by release and blur could survive
as a queued movement pulse. The focused Rust reproduction failed with x3 instead
of x2; explicit cancellation fixes it. Shipped browser handler tests cover all
three pages, including independent pointer cancellation. Existing RPG and arena
checksum tests passed unchanged.

All 21 implementation-plan gates passed for this input increment, including
native and actual-WASM control loops, externally copied starter and relocated
macOS bundles. Focused browser tests run with:

```sh
node --test web/shared/input.test.mjs
```
