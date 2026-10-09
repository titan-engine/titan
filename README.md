# Titan

**An experimental, AI-native game engine built on [Bevy](https://bevy.org).**

Titan is an independent downstream distribution of Bevy. Most of the engine is
Bevy's work, and we carry its full git history. What Titan adds is a different
way of developing it: AI-assisted and agent-authored contributions are welcome
here, and the goal is to push what AI-driven game development can do.

> [!NOTE]
> Titan is not affiliated with or endorsed by the Bevy project or the Bevy
> Foundation. For a stable, community-supported engine, use
> [Bevy](https://github.com/bevyengine/bevy). Please don't report Titan bugs
> upstream unless you've reproduced them on Bevy itself.

## Why Titan?

Rust's compiler and tooling give agents fast, precise feedback, and Bevy is the
strongest Rust game engine. Titan is an experiment in how far that combination
goes when the engine and its workflow are built for agents:

- **Shorter loops from idea to working game.** Agents should be able to build,
  run, inspect, and fix a game without a human in the middle.
- **Observability.** Expose entities, components, resources, assets, and
  schedules through structured, machine-readable interfaces.
- **Reproducible gameplay tests.** Scripted input, fixed seeds, controlled
  ticks, and assertions about game state.
- **Diagnostics an agent can act on.** Say *which* entity, asset, system, or
  scene was involved, not just that something failed.

## Relationship to Bevy

- **Baseline:** Titan tracks stable Bevy releases. It is currently based on
  **Bevy 0.20.0**. See [MAINTAINING.md](MAINTAINING.md) for how we sync.
- **Crate names:** Crates keep their `bevy_*` names so upstream merges stay
  cheap. Titan is not published to crates.io; depend on it via git (see below).
- **Upstreaming:** When a Titan change makes sense for Bevy, we offer it
  upstream as a focused patch that follows
  [Bevy's contribution and AI policies](https://bevy.org/learn/contribute/policies/ai/).
  It's up to Bevy what they accept.

## Getting Started

Titan is API-compatible with the Bevy release it is based on, so Bevy's
[Quick Start Guide](https://bevy.org/learn/quick-start/introduction),
[API docs](https://docs.rs/bevy/0.20.0/bevy/), and
[examples](examples) apply.

```toml
[dependencies]
bevy = { git = "https://github.com/titan-engine/titan", branch = "main" }
```

Pin a commit with `rev = "..."` if you need reproducible builds.

```sh
# Run an example from a checkout of this repository
cargo run --example breakout
```

## Contributing

AI-assisted contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md)
for the policy and [AGENTS.md](AGENTS.md) for instructions aimed at coding agents.

## Thanks

Titan exists because of the work of [Bevy's contributors](https://github.com/bevyengine/bevy/graphs/contributors)
and sponsors. If Titan is useful to you, please consider
[supporting Bevy](https://bevy.org/donate/).

## License

Titan inherits Bevy's licensing. Except where noted (below and/or in individual files), all code in this repository is dual-licensed under either:

- MIT License ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

at your option.
This means you can select the license you prefer!
This dual-licensing approach is the de-facto standard in the Rust ecosystem and there are [very good reasons](https://github.com/bevyengine/bevy/issues/2373) to include both.

Some of the engine's code carries additional copyright notices and license terms due to their external origins.
These are generally BSD-like, but exact details vary by crate:
If the README of a crate contains a 'License' header (or similar), the additional copyright notices and license terms applicable to that crate will be listed.
The above licensing requirement still applies to contributions to those crates, and sections of those crates will carry those license terms.
The [license](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields) field of each crate will also reflect this.

The [assets](assets) included in this repository (for our [examples](./examples/README.md)) typically fall under different open licenses.
These will not be included in your game (unless copied in by you), and they are not distributed in the published Bevy crates.
See [CREDITS.md](CREDITS.md) for the details of the licenses of those files.

### Your contributions

Unless you explicitly state otherwise,
any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license,
shall be dual licensed as above,
without any additional terms or conditions.
