#!/usr/bin/env python3
"""Security and lifecycle checks for bounded native acceptance evidence."""
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zlib

from acceptance_evidence import ALLOWLIST, FailureEvidence, IMAGE_LIMIT, JSON_LIMIT, TEXT_LIMIT, TOTAL_LIMIT


def png():
    def chunk(kind, value):
        return struct.pack(">I", len(value)) + kind + value + struct.pack(">I", zlib.crc32(kind + value))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(b"\0\xff\0\0\xff")) + chunk(b"IEND", b""))


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.destination = self.root / "failures"
        self.env = patch.dict(os.environ, {"TITAN_ACCEPTANCE_EVIDENCE_DIR": str(self.destination)})
        self.env.start()
        self.addCleanup(self.env.stop)

    def fail_evidence(self, evidence):
        try:
            with evidence:
                raise AssertionError("original acceptance failure")
        except AssertionError as error:
            self.assertEqual(str(error), "original acceptance failure")
        return evidence.output_dir

    def make_bundle(self):
        bundle = self.root / "diagnostics"
        bundle.mkdir(exist_ok=True)
        (bundle / "bundle.json").write_text(json.dumps({"token": "registration-secret", "capture": {"artifact": "capture.png"}, "error": "Bearer inline-secret", "state": {"count": 4}}))
        (bundle / "api.txt").write_text("allowed operation\npassword=api-secret\n")
        (bundle / "capture.png").write_bytes(png())
        (bundle / "discovery.json").write_text('{"token":"never-read"}')
        return bundle

    def test_snapshot_survives_cleanup_and_is_allowlisted_sanitized(self):
        bundle = self.make_bundle()
        evidence = FailureEvidence("test-rpg")
        evidence.redact_secret("bare-secret")
        evidence.record_command(["titan", "--token", "argument-secret"], subprocess.CompletedProcess([], 1, 'Authorization: Bearer output-secret\nbare-secret', '{"password":"stderr-secret"}'))
        evidence.observe({"error": {"details": {"diagnostic_bundle": str(bundle / "bundle.json")}}})
        (bundle / "bundle.json").unlink()
        direct = self.root / "capture.ppm"
        direct.write_bytes(b"P6\n1 1\n255\n\xff\x00\x00")
        evidence.observe({"response": {"artifact": str(direct)}})
        direct.unlink()
        output = self.fail_evidence(evidence)
        self.assertTrue({p.name for p in output.iterdir()} <= ALLOWLIST)
        self.assertEqual((output / "capture.png").read_bytes(), png())
        self.assertTrue((output / "latest-capture.ppm").is_file())
        joined = b"".join(p.read_bytes() for p in output.iterdir())
        for secret in (b"registration-secret", b"inline-secret", b"api-secret", b"never-read", b"argument-secret", b"output-secret", b"stderr-secret", b"bare-secret"):
            self.assertNotIn(secret, joined)
        self.assertEqual(json.loads((output / "bundle.json").read_text())["state"]["count"], 4)
        context = json.loads((output / "context.json").read_text())
        self.assertIn("original acceptance failure", context["traceback"])
        self.assertIn("test-acceptance-evidence.py", context["traceback"])

    def test_passing_context_has_no_staging_or_export(self):
        with FailureEvidence("pass") as evidence:
            evidence.observe({"error": {"details": {"diagnostic_bundle": str(self.make_bundle() / "bundle.json")}}})
        self.assertFalse(self.destination.exists())
        self.assertEqual(evidence.files, {})

    def test_repeated_failures_have_distinct_private_directories(self):
        first = self.fail_evidence(FailureEvidence("../../test"))
        second = self.fail_evidence(FailureEvidence("../../test"))
        self.assertNotEqual(first, second)
        self.assertEqual(first.parent, self.destination)
        self.assertEqual(first.stat().st_mode & 0o777, 0o700)
        self.assertTrue(all(p.stat().st_mode & 0o777 == 0o600 for p in first.iterdir()))

    def test_collection_failure_preserves_exception(self):
        evidence = FailureEvidence("fail")
        with patch.object(evidence, "_export", side_effect=OSError("broken disk")):
            self.fail_evidence(evidence)
        self.assertEqual(evidence.files, {})

    def test_runtime_pipe_drains_noisy_child_with_bounded_memory_and_disk(self):
        evidence = FailureEvidence("noisy")
        with evidence.runtime_log() as log:
            process = subprocess.Popen([sys.executable, "-c", "import sys; sys.stdout.write('x' * 4000000 + '\\nfinal line\\n'); sys.stdout.flush()"], stdout=log, stderr=log)
            evidence.record_process(process)
            process.wait(timeout=10)
        self.assertLessEqual(len(evidence.files["runtime.log"]), TEXT_LIMIT)
        self.assertIn(b"final line", evidence.files["runtime.log"])
        output = self.fail_evidence(evidence)
        self.assertEqual(json.loads((output / "context.json").read_text())["process_ids"], [process.pid])
        self.assertLessEqual(sum(p.stat().st_size for p in output.iterdir()), TOTAL_LIMIT)

    def test_runtime_truncation_drops_secret_fragment_without_its_label(self):
        evidence = FailureEvidence("runtime-secret")
        with evidence.runtime_log() as log:
            process = subprocess.Popen([sys.executable, "-c", "print('token=' + 's' * 150000); print('last-output')"], stdout=log, stderr=log)
            process.wait(timeout=10)
        output = self.fail_evidence(evidence)
        runtime = (output / "runtime.log").read_text()
        self.assertIn("last-output", runtime)
        self.assertNotIn("s" * 100, runtime)

    def test_symlink_components_traversal_fifo_and_large_files_rejected(self):
        bundle = self.make_bundle()
        linked = self.root / "linked"
        linked.symlink_to(bundle, target_is_directory=True)
        fifo = self.root / "fifo"
        fifo.mkdir()
        os.mkfifo(fifo / "bundle.json")
        huge = self.root / "huge"
        huge.mkdir()
        with (huge / "bundle.json").open("wb") as stream:
            stream.truncate(JSON_LIMIT + 1)
        for path in (linked / "bundle.json", bundle / ".." / "diagnostics" / "bundle.json", fifo / "bundle.json", huge / "bundle.json"):
            with self.subTest(path=path):
                evidence = FailureEvidence("invalid")
                evidence.observe({"error": {"details": {"diagnostic_bundle": str(path)}}})
                output = self.fail_evidence(evidence)
                self.assertFalse((output / "bundle.json").exists())
                self.assertTrue(json.loads((output / "context.json").read_text())["collection_errors"])

    def test_malicious_capture_reference_is_ignored_and_bad_images_rejected(self):
        bundle = self.make_bundle()
        (bundle / "bundle.json").write_text('{"capture":{"artifact":"../../discovery.json"}}')
        (bundle / "capture.png").write_bytes(png() + b"token=secret-trailer")
        evidence = FailureEvidence("bad-capture")
        evidence.observe({"error": {"details": {"diagnostic_bundle": str(bundle / "bundle.json")}}})
        direct = self.root / "capture.ppm"
        direct.write_bytes(b"P6\n1 1\n255\nabcsecret")
        evidence.observe({"response": {"artifact": str(direct)}})
        output = self.fail_evidence(evidence)
        self.assertFalse((output / "capture.png").exists())
        self.assertFalse((output / "latest-capture.ppm").exists())
        self.assertNotIn(b"secret-trailer", b"".join(p.read_bytes() for p in output.iterdir()))

    def test_deep_json_and_large_capture_are_bounded(self):
        bundle = self.make_bundle()
        (bundle / "bundle.json").write_text("[" * 3000 + "0" + "]" * 3000)
        evidence = FailureEvidence("deep")
        evidence.observe({"error": {"details": {"diagnostic_bundle": str(bundle / "bundle.json")}}})
        direct = self.root / "capture.ppm"
        with direct.open("wb") as stream:
            stream.truncate(IMAGE_LIMIT + 1)
        evidence.observe({"response": {"artifact": str(direct)}})
        output = self.fail_evidence(evidence)
        self.assertEqual(set(p.name for p in output.iterdir()), {"context.json", "commands.log"})

    def test_checkpoint_matches_only_selected_test_and_checkpoint(self):
        evidence = FailureEvidence("rpg")
        with patch.dict(os.environ, {"TITAN_ACCEPTANCE_FAIL": "arena"}):
            evidence.checkpoint("diagnostics")
        with patch.dict(os.environ, {"TITAN_ACCEPTANCE_FAIL": "rpg:diagnostics"}):
            evidence.checkpoint("capture")
            with self.assertRaisesRegex(AssertionError, "rpg:diagnostics"):
                evidence.checkpoint("diagnostics")
        with patch.dict(os.environ, {"TITAN_ACCEPTANCE_FAIL": "rpg"}):
            with self.assertRaises(AssertionError):
                evidence.checkpoint("capture")

    def test_recent_command_tail_is_bounded_and_secrets_redacted_before_truncation(self):
        evidence = FailureEvidence("tail")
        for _ in range(20):
            evidence.record_command(["tool"], subprocess.CompletedProcess([], 2, 'token="' + 's' * (TEXT_LIMIT * 2) + '"\nlast-output', 'last-error'))
        self.assertLessEqual(len(evidence.commands), TEXT_LIMIT)
        output = self.fail_evidence(evidence)
        logs = (output / "commands.log").read_text()
        self.assertIn("last-output", logs)
        self.assertNotIn("s" * 100, logs)


if __name__ == "__main__":
    unittest.main()
