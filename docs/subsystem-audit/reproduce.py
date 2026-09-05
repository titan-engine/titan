#!/usr/bin/env python3
"""Run the issue #38 investigation in a temporary external Cargo workspace."""
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "scripts"))
import acceptance_process as processes

OUTPUT = REPO / "target" / "subsystem-audit"


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="titan-ecs-consumer-") as directory:
        project = Path(directory)
        (project / "src").mkdir()
        (project / "Cargo.toml").write_text(
            '[package]\nname = "ecs-consumer"\nversion = "0.0.0"\n'
            'edition = "2024"\npublish = false\n\n[workspace]\n\n'
            '[lib]\ncrate-type = ["rlib", "cdylib"]\n\n'
            '[features]\ndefault = ["titan/image-png"]\n\n'
            f'[dependencies]\ntitan = {{ path = {json.dumps(str(REPO))}, '
            'default-features = false }\n'
        )
        shutil.copyfile(Path(__file__).with_name("consumer.rs"), project / "src/lib.rs")
        (project / "src/main.rs").write_text(
            'fn main() {\n    assert_eq!(ecs_consumer::ecs_probe(), 42);\n'
            '    println!("ECS probe passed: 42");\n}\n'
        )
        # Seed the external resolution with the audited checkout's versions.
        shutil.copyfile(REPO / "Cargo.lock", project / "Cargo.lock")

        def run(args, *, capture=False):
            return processes.run(args, cwd=project, check=True, text=True,
                                 env=dict(os.environ, CARGO_TARGET_DIR=str(project / "target")),
                                 capture_output=capture, phase="build")

        run(["cargo", "check", "--offline", "--no-default-features"])
        shutil.copyfile(project / "Cargo.lock", OUTPUT / "consumer.Cargo.lock")
        run(["cargo", "fmt", "--all", "--check"])
        for label, flags in [("png-free", ["--no-default-features"]), ("png", [])]:
            run(["cargo", "run", "--locked", *flags])
            run(["cargo", "clippy", "--locked", "--all-targets", *flags,
                 "--", "-D", "warnings"])
            for target in ["native", "wasm32-unknown-unknown"]:
                target_flags = [] if target == "native" else ["--target", target]
                tree = run(["cargo", "tree", "--locked", "-e", "normal,build,features",
                            *target_flags, *flags], capture=True).stdout
                (OUTPUT / f"{label}-{target}.txt").write_text(tree)
            run(["cargo", "build", "--locked", "--lib", "--target",
                 "wasm32-unknown-unknown", *flags])
            # No wasm-bindgen, browser host, GPU, or WASI imports are needed.
            run(["node", "--input-type=module", "-e", """
import fs from 'node:fs';
const module = new WebAssembly.Module(fs.readFileSync(process.argv[1]));
if (WebAssembly.Module.imports(module).length !== 0) throw Error('unexpected imports');
const instance = new WebAssembly.Instance(module, {});
if (instance.exports.ecs_probe() !== 42) throw Error('ECS probe failed');
console.log('WASM ECS probe passed: 42; imports: 0');
""", str(project / "target/wasm32-unknown-unknown/debug/ecs_consumer.wasm")])
        print(f"Dependency evidence: {OUTPUT}")


if __name__ == "__main__":
    main()
