# RPG demo fixture

`titan-rpg` is the internal, non-published owner of Titan's existing RPG game,
procedural and PNG-backed art, quest journal, snapshots and live replay state.
It is a regression fixture and demo, not an engine API or a new-game framework.
Start new games from the [standalone starter](../../starters/minimal/README.md).

The headless `procedural_rpg`, native `play_rpg` and verifier `replay_rpg`
examples, `titan-browser`, and renderer offscreen tests depend on this package.
Their existing Cargo commands and asset lookup rules are unchanged. Hosts own
windowing, discovery/transports, browser exports, packaging and their integration
tests. Game unit tests live here and run once with `cargo test -p titan-rpg`;
source includes no longer repeat them in each adapter's test binary.

The library's normal dependency edges are `titan-rpg -> titan` and
`titan-rpg -> titan-protocol` (plus serialization). It has no browser, renderer,
remote-host or diagnostics dependency. The root `titan` examples and renderer
tests use dev-dependency edges to this fixture; those do not create a library
compilation cycle. `titan-browser` uses a normal dependency and separately selects
its WASM renderer. The fixture uses Titan's existing PNG feature; native file I/O
stays target-gated, while image decoding/game state compile on actual WASM.
`cargo check -p titan --lib --no-default-features` remains independent of this
fixture and PNG support.

## Inspection compatibility

Before extraction, each source-including target gave the same component a
different Rust type name. Hosts now call `register_legacy_component_names` before
exposing their inspector, preserving these existing protocol prefixes:

| Adapter | Existing component prefix |
| --- | --- |
| Headless example | `procedural_rpg::game::` |
| Native player | `play_rpg::game::` |
| Browser runtime and player | `titan_browser::game::` |

The inventoried components are `Position`, `Player`, `Shard`, `Shrine`,
`ActiveShrine`, `QuestHud` and `journal::JournalNode`. Aliases preserve entity
lists/details, component query filters, field metadata, `set_field` requests and
diagnostic world/API summaries. Built-in Titan/UI component names are unchanged.
The verifier and offscreen test previously used `replay_rpg::game::` and
`offscreen::game::`; neither exposes an inspector to external clients. The
fixture's compatibility helper also supports those prefixes.

Direct Rust `World`/`App::system_metadata` values now describe the true defining
package (`titan_rpg`). This is an internal source-ownership change, not a protocol
identifier migration. The affected game system names are `setup`,
`live::begin_recording`, `apply_scheduled_input`, `live::record_consumed`,
`move_player`, `collect_shards`, `activate_shrine`, `sync_quest_ui`,
`journal::sync_labels` and `live::finish_tick`; `ApplyDeferred` is unchanged.
Typed access metadata also contains `QuestState`, `journal::Journal` and
`InputFrame<Action>`. The RPG has no fallible typed systems requiring diagnostic
system-error name translation. Snapshots and recordings serialize explicit game
fields, never these Rust identifiers, so their payloads and formats are unchanged.

The exact eleven-tick reference remains `f7a298f62ad75c1c`; reference images and
the committed README preview are unchanged. See the [quality gates](../../docs/implementation-plan.md)
for native/WASM, replay, asset and opt-in GPU checks.
