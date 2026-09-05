#!/usr/bin/env python3
"""Run the deterministic PNG corpus and mutations in a resource-bounded child."""
import argparse
import json
import os
from pathlib import Path
import resource
import subprocess
import sys
import time

import acceptance_process as processes

ROOT = Path(__file__).resolve().parents[1]


def limited_exec(command):
    # Apply limits after Cargo has finished; never limit compiler address space.
    resource.setrlimit(resource.RLIMIT_CPU, (30, 30))
    resource.setrlimit(resource.RLIMIT_FSIZE, (8 * 1024 * 1024,) * 2)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    if sys.platform.startswith("linux"):
        resource.setrlimit(resource.RLIMIT_AS, (1024 * 1024 * 1024,) * 2)
    os.execv(command[0], command)


def run_bounded(command, output, timeout=60):
    """The decoder is single-process; macOS RSS monitoring is sampled, not a hard cap."""
    with output.open("wb") as log:
        with processes.Popen([sys.executable, str(Path(__file__).resolve()),
                              "--limited-exec", *map(str, command)],
                             cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                             timeout=timeout) as child:
            while child.poll() is None:
                if sys.platform == "darwin":
                    sample = subprocess.run(["ps", "-o", "rss=", "-p", str(child.pid)],
                                            capture_output=True, text=True, timeout=2)
                    if sample.returncode == 0 and sample.stdout.strip():
                        if int(sample.stdout.strip()) > 512 * 1024:
                            raise RuntimeError("PNG child exceeded 512 MiB sampled RSS")
                time.sleep(0.02)
            if child.returncode:
                raise RuntimeError(f"PNG child exited with status {child.returncode}; see {output}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=int, default=69)
    parser.add_argument("--iterations", type=int, default=1000)
    parser.add_argument("--corpus", type=Path, default=ROOT / "fixtures/png-corpus")
    parser.add_argument("--replay", type=Path)
    args = parser.parse_args()
    if not 0 <= args.seed < 2**64 or not 0 <= args.iterations <= 1_000_000:
        parser.error("seed must fit u64; iterations must be in 0..1000000")
    if sys.platform not in ("linux", "darwin"):
        parser.error("resource containment supports Linux and macOS only")
    processes.run(["cargo", "build", "--locked", "--example", "png_fuzz"],
                  cwd=ROOT, phase="build", check=True)
    metadata = json.loads(processes.check_output(
        ["cargo", "metadata", "--format-version=1", "--no-deps"], cwd=ROOT, phase="build"))
    target = Path(metadata["target_directory"])
    evidence_root = target / "png-fuzz"
    evidence_root.mkdir(parents=True, exist_ok=True)
    # Separate runs preserve failures and avoid clobbering concurrent campaigns.
    import tempfile
    evidence = Path(tempfile.mkdtemp(prefix="run-", dir=evidence_root))
    artifact = evidence / "current.json"
    command = [str(target / "debug/examples/png_fuzz"), "--seed", str(args.seed),
               "--iterations", str(args.iterations), "--corpus", str(args.corpus.resolve()),
               "--artifact", str(artifact)]
    if args.replay:
        command.extend(["--replay", str(args.replay.resolve())])
    (evidence / "run.json").write_text(json.dumps({
        "seed": args.seed, "iterations": args.iterations, "command": command,
        "wall_seconds": 60, "cpu_seconds": 30,
        "linux_address_space_bytes": 1024**3, "macos_sampled_rss_bytes": 512 * 1024**2,
        "file_bytes": 8 * 1024**2, "platform": sys.platform,
    }, indent=2) + "\n")
    try:
        run_bounded(command, evidence / "run.log")
    except (RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"FAIL: {error}\nEvidence: {evidence}", file=sys.stderr)
        if artifact.exists():
            print(f"Replay: python3 scripts/fuzz-png.py --replay {artifact}", file=sys.stderr)
        return 1
    print((evidence / "run.log").read_text(), end="")
    # Successful campaigns need no retained input/log artifacts.
    import shutil
    shutil.rmtree(evidence)
    return 0


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--limited-exec":
        limited_exec(sys.argv[2:])
    else:
        raise SystemExit(main())
