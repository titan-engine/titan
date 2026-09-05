#!/usr/bin/env python3
"""Build and stage the public demos; only explicitly listed files are published."""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
# These are public web assets, never a recursive copy of web/ or target/.
COMMON = (
    "play/index.html", "play/play.js", "play/replay.mjs",
    "inspector/index.html", "inspector/inspector.js", "inspector/inspector.css",
    "inspector/bridge.mjs", "shared/input.mjs",
)
RPG = COMMON + (
    "play/journal.mjs", "shared/player-asset.mjs",
    "assets/player.png", "assets/tree.png",
    "inspector/pkg/titan_browser.js", "inspector/pkg/titan_browser_bg.wasm",
)
ARENA = COMMON + (
    "play/entities.mjs", "play/pointer.mjs", "play/save.mjs",
    "inspector/pkg/titan_game.js", "inspector/pkg/titan_game_bg.wasm",
)
COLLECTION_ROOM = (
    "play/index.html", "play/play.js", "play/keys.mjs",
    "inspector/pkg/titan_game.js", "inspector/pkg/titan_game_bg.wasm",
)


def reject_symlink_path(path):
    """Reject symlinks anywhere in a checkout-relative publication path."""
    relative = path.relative_to(ROOT)
    current = ROOT
    for part in relative.parts:
        current = current / part
        if current.is_symlink():
            raise ValueError(f"symlinked publication path: {current}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true", help="stage existing browser builds")
    args = parser.parse_args(argv)
    if not args.no_build:
        for script in ("scripts/build-browser.py", "games/arena/scripts/build-browser.py",
                       "games/collection-room/scripts/build-browser.py"):
            subprocess.run([sys.executable, str(ROOT / script)], cwd=ROOT, check=True)

    target = ROOT / "target"
    reject_symlink_path(target)
    target.mkdir(exist_ok=True)
    output = target / "pages"
    if output.is_symlink() or (output.exists() and not output.is_dir()):
        raise ValueError(f"refusing to replace non-directory output: {output}")
    with tempfile.TemporaryDirectory(prefix=".pages-", dir=target) as temporary:
        stage = Path(temporary) / "site"
        stage.mkdir()
        files = [(ROOT / "docs/site/index.html", "index.html"),
                 (ROOT / "LICENSE-MIT", "LICENSE-MIT"),
                 (ROOT / "LICENSE-APACHE", "LICENSE-APACHE")]
        for source, prefix, paths in ((ROOT / "web", "rpg", RPG),
                                      (ROOT / "games/arena/web", "arena", ARENA),
                                      (ROOT / "games/collection-room/web", "collection-room", COLLECTION_ROOM)):
            files.extend((source / path, f"{prefix}/{path}") for path in paths)
        for source, relative in files:
            reject_symlink_path(source)
            if not source.is_file():
                raise ValueError(f"missing public asset: {source}")
            destination = stage / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
        (stage / ".nojekyll").touch()
        if output.exists():
            shutil.rmtree(output)
        stage.rename(output)
    print(f"Staged {len(files) + 1} public files at {output}")
    print("Preview: python3 -m http.server 8000 --bind 127.0.0.1 --directory target")
    print("Open http://127.0.0.1:8000/pages/ (also checks a non-root URL prefix).")


if __name__ == "__main__":
    main()
