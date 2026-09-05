# RPG player sprite

`player.png` is the exact RGBA8 export of the RPG's existing procedural player
(8×10 pixels). It shares the source artwork's project license. Regenerate it with:

```sh
cargo run --example procedural_rpg -- --export-player-png assets/player.png
```

Native/browser hosts load this image at startup. Replacing it and restarting the
process or reloading the browser changes the sprite without recompiling Rust.
`python3 scripts/build-browser.py` copies it into `web/assets`; during browser
iteration, edit that served copy directly and reload. The next build replaces
the served copy from this directory. Keep source edits here for persistence.
The native app builder places it in `Contents/Resources/assets`.

`--generated-assets` selects the procedural comparison on native runners. In-game
restart and replay retain the startup image; they do not reread files. See
[asset behavior and limits](../docs/assets.md).
