"""Bounded, allowlisted failure evidence for native acceptance harnesses.

Artifacts are snapshotted in memory when observed, before temporary projects die.
Only a failed context writes a unique private directory; collection never masks
its original exception. This module deliberately never reads discovery files.
"""
from contextlib import contextmanager
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import select
import stat
import struct
import sys
import tempfile
import threading
import traceback as traceback_module
import zlib

TEXT_LIMIT = 128 * 1024
JSON_LIMIT = 512 * 1024
IMAGE_LIMIT = 2 * 1024 * 1024
TOTAL_LIMIT = 6 * 1024 * 1024
ALLOWLIST = frozenset({"context.json", "commands.log", "runtime.log", "bundle.json",
                       "api.txt", "capture.png", "latest-capture.ppm"})
SECRET_KEY = re.compile(r"token|password|secret|authorization|credential|cookie", re.I)
BEARER = re.compile(r"(?i)\bbearer\s+[^\s\"',;}]+")
ASSIGNMENT = re.compile(
    r'''(?ix)(\b["']?[\w-]*(?:token|password|secret|authorization|credential|cookie)[\w-]*["']?\s*[:=]\s*)(?:"[^"\n]*"|'[^'\n]*'|[^\s,;}]+)''')
OPTION = re.compile(r"(?i)(--[\w-]*(?:token|password|secret|authorization|credential|cookie)[\w-]*\s+)(\S+)")


def sanitize(value):
    """Remove sensitive JSON fields and common text credential forms."""
    if isinstance(value, dict):
        return {str(k): "[REDACTED]" if SECRET_KEY.search(str(k)) else sanitize(v)
                for k, v in value.items()}
    if isinstance(value, list):
        return [sanitize(item) for item in value]
    if isinstance(value, str):
        return OPTION.sub(r"\1[REDACTED]", ASSIGNMENT.sub(r"\1[REDACTED]", BEARER.sub("Bearer [REDACTED]", value)))
    return value


def _text(value):
    if value is None:
        return ""
    if isinstance(value, bytes):
        value = value.decode("utf-8", errors="replace")
    return str(value)


def _read_regular(path, limit):
    """Open each component without following symlinks, and never read a FIFO."""
    path = Path(path)
    if not path.is_absolute() or ".." in path.parts:
        raise ValueError("artifact path must be absolute without traversal")
    descriptor = os.open(path.anchor, os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.parts[1:-1]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        source = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=descriptor)
        try:
            info = os.fstat(source)
            if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
                raise ValueError("artifact is not a bounded regular file")
            with os.fdopen(source, "rb", closefd=False) as stream:
                data = stream.read(limit + 1)
            if len(data) > limit:
                raise ValueError("artifact exceeded limit while reading")
            return data
        finally:
            os.close(source)
    finally:
        os.close(descriptor)


def _png(data):
    """Keep only valid native RGBA PNG image chunks; reject metadata/trailers."""
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("invalid PNG signature")
    offset, compressed, dimensions, ended = 8, bytearray(), None, False
    while offset < len(data):
        if offset + 12 > len(data):
            raise ValueError("truncated PNG")
        length = struct.unpack(">I", data[offset:offset + 4])[0]
        kind = data[offset + 4:offset + 8]
        end = offset + 12 + length
        if end > len(data) or kind not in (b"IHDR", b"IDAT", b"IEND"):
            raise ValueError("PNG contains non-allowlisted chunks")
        payload = data[offset + 8:end - 4]
        if zlib.crc32(kind + payload) != struct.unpack(">I", data[end - 4:end])[0]:
            raise ValueError("invalid PNG checksum")
        if kind == b"IHDR":
            if dimensions is not None or offset != 8 or length != 13:
                raise ValueError("invalid PNG header")
            width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", payload)
            if not width or not height or width * height * 4 > IMAGE_LIMIT or (depth, color, compression, filtering, interlace) != (8, 6, 0, 0, 0):
                raise ValueError("unsupported or oversized PNG")
            dimensions = (width, height)
        elif kind == b"IDAT":
            if dimensions is None:
                raise ValueError("missing PNG header")
            compressed.extend(payload)
        else:
            if length or end != len(data) or dimensions is None:
                raise ValueError("invalid PNG end")
            ended = True
        offset = end
    if not ended:
        raise ValueError("missing PNG end")
    decoder = zlib.decompressobj()
    expected = (dimensions[0] * 4 + 1) * dimensions[1]
    decoded = decoder.decompress(bytes(compressed), expected + 1)
    if len(decoded) != expected or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
        raise ValueError("invalid PNG image stream")
    return data


def _ppm(data):
    header = re.match(rb"P6\n([0-9]{1,5}) ([0-9]{1,5})\n255\n", data)
    if not header:
        raise ValueError("unsupported PPM")
    width, height = map(int, header.groups())
    if not width or not height or width * height * 3 + header.end() != len(data):
        raise ValueError("invalid PPM dimensions or trailer")
    return data


class _RuntimeLog:
    def __init__(self):
        self.reader, self.writer = os.pipe()
        self.tail = bytearray()
        self.truncated = False
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._drain, daemon=True)
        self.thread.start()

    def _drain(self):
        try:
            while True:
                readable, _, _ = select.select([self.reader], [], [], 0.05)
                if not readable:
                    if self.stop.is_set():
                        break
                    continue
                chunk = os.read(self.reader, 8192)
                if not chunk:
                    break
                with self.lock:
                    self.tail.extend(chunk)
                    if len(self.tail) > TEXT_LIMIT:
                        self.truncated = True
                        del self.tail[:-TEXT_LIMIT]
                if self.stop.is_set():
                    break
        finally:
            os.close(self.reader)

    def fileno(self):
        return self.writer

    def seek(self, offset):
        if offset != 0:
            raise ValueError("bounded runtime log only supports seek(0)")

    def read(self):
        with self.lock:
            data = bytes(self.tail)
            if self.truncated:
                # A removed prefix may have contained a credential label. Never
                # export the surviving fragment of that first truncated line.
                _, separator, data = data.partition(b"\n")
                if not separator:
                    data = b""
                data = b"[earlier runtime output truncated]\n" + data
            return data.decode("utf-8", errors="replace")

    def close(self):
        os.close(self.writer)
        # Normally process cleanup has closed all inherited writers. Never block
        # evidence export on a descendant that inherited stdout.
        self.thread.join(0.5)
        if self.thread.is_alive():
            self.stop.set()
            self.thread.join(0.2)


class FailureEvidence:
    def __init__(self, name, repo=None):
        self.name = name
        self.repo = Path(repo or Path(__file__).resolve().parent.parent)
        self.files = {}
        self.commands = ""
        self.collection_errors = []
        self.output_dir = None
        self.pids = []
        self.secrets = set()

    def __enter__(self):
        return self

    def _error(self, error):
        self.collection_errors.append(type(error).__name__)
        self.collection_errors = self.collection_errors[-16:]

    def redact_secret(self, value):
        if isinstance(value, str) and value:
            self.secrets.add(value)

    def record_process(self, process):
        self.pids.append(process.pid)
        self.pids = self.pids[-16:]

    def _sanitize(self, value):
        text = _text(value)
        try:
            text = json.dumps(sanitize(json.loads(text)), indent=2)
        except (ValueError, RecursionError):
            text = sanitize(text)
        for secret in self.secrets:
            text = text.replace(secret, "[REDACTED]")
        return text

    def _put_text(self, name, value, limit=TEXT_LIMIT):
        self.files[name] = self._sanitize(value).encode("utf-8")[-limit:]

    def record_command(self, args, result):
        try:
            arguments = [str(arg) for arg in args]
            for index, argument in enumerate(arguments):
                if index and SECRET_KEY.search(arguments[index - 1]) and arguments[index - 1].startswith("--"):
                    arguments[index] = "[REDACTED]"
            entry = json.dumps({"command": sanitize(arguments),
                                "returncode": getattr(result, "returncode", None),
                                "stdout": self._sanitize(getattr(result, "stdout", ""))[-TEXT_LIMIT:],
                                "stderr": self._sanitize(getattr(result, "stderr", ""))[-TEXT_LIMIT:]}, ensure_ascii=True)
            self.commands = (self.commands + entry + "\n")[-TEXT_LIMIT:]
        except Exception as error:
            self._error(error)

    def observe(self, response):
        """Snapshot only protocol-declared diagnostics and direct capture paths."""
        try:
            if not isinstance(response, dict):
                return
            details = response.get("error", {}).get("details", {})
            manifest = details.get("diagnostic_bundle") if isinstance(details, dict) else None
            if isinstance(manifest, str):
                path = Path(manifest)
                if path.name != "bundle.json":
                    raise ValueError("unexpected diagnostic manifest name")
                raw = _read_regular(path, JSON_LIMIT)
                bundle = json.loads(raw)
                if not isinstance(bundle, dict):
                    raise ValueError("diagnostic manifest must be an object")
                encoded = json.dumps(sanitize(bundle), indent=2).encode()
                if len(encoded) > JSON_LIMIT:
                    raise ValueError("sanitized manifest exceeds limit")
                self.files["bundle.json"] = encoded
                for name in ("api.txt", "capture.png"):
                    self.files.pop(name, None)
                    try:
                        raw = _read_regular(path.parent / name, IMAGE_LIMIT if name.endswith("png") else TEXT_LIMIT)
                        if name.endswith("png"):
                            self.files[name] = _png(raw)
                        else:
                            self._put_text(name, raw)
                    except FileNotFoundError:
                        pass
                    except Exception as error:
                        self._error(error)
            capture = response.get("response", {})
            if isinstance(capture, dict) and isinstance(capture.get("artifact"), str):
                path = Path(capture["artifact"])
                if path.name != "capture.ppm":
                    raise ValueError("unexpected direct capture name")
                self.files["latest-capture.ppm"] = _ppm(_read_regular(path, IMAGE_LIMIT))
        except Exception as error:
            self._error(error)

    @contextmanager
    def runtime_log(self):
        log = _RuntimeLog()
        try:
            yield log
        finally:
            try:
                log.close()
                self._put_text("runtime.log", log.read())
            except Exception as error:
                self._error(error)

    def checkpoint(self, name):
        if os.environ.get("TITAN_ACCEPTANCE_FAIL") in (self.name, f"{self.name}:{name}"):
            raise AssertionError(f"controlled acceptance failure: {self.name}:{name}")

    def _export(self, error, traceback=None):
        self.files["commands.log"] = self.commands.encode()[-TEXT_LIMIT:]
        context = {"test": self.name, "utc": datetime.now(timezone.utc).isoformat(),
                   "exception": type(error).__name__,
                   "traceback": self._sanitize("".join(traceback_module.format_exception(type(error), error, traceback)))[-TEXT_LIMIT // 4:], "message": self._sanitize(str(error))[:TEXT_LIMIT // 4],
                   "collection_errors": self.collection_errors, "process_ids": self.pids,
                   "limits": {"text_bytes": TEXT_LIMIT, "json_bytes": JSON_LIMIT,
                              "image_bytes": IMAGE_LIMIT, "total_bytes": TOTAL_LIMIT}}
        self.files["context.json"] = json.dumps(context, indent=2).encode()
        root = Path(os.environ.get("TITAN_ACCEPTANCE_EVIDENCE_DIR", self.repo / "target" / "acceptance-failures"))
        root.mkdir(parents=True, exist_ok=True)
        safe_name = re.sub(r"[^a-zA-Z0-9_-]", "-", self.name)[:64] or "acceptance"
        self.output_dir = Path(tempfile.mkdtemp(prefix=safe_name + "-", dir=root))
        total = 0
        for name, data in self.files.items():
            if name not in ALLOWLIST:
                continue
            if not name.endswith((".png", ".ppm")):
                for secret in self.secrets:
                    data = data.replace(secret.encode(), b"[REDACTED]")
                if name not in ("bundle.json", "context.json"):
                    data = data[-TEXT_LIMIT:]
            limit = JSON_LIMIT if name.endswith(".json") else IMAGE_LIMIT if name.endswith((".png", ".ppm")) else TEXT_LIMIT
            if len(data) > limit or total + len(data) > TOTAL_LIMIT:
                continue
            descriptor = os.open(self.output_dir / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, "wb") as stream:
                stream.write(data)
            total += len(data)
        print(f"Acceptance failure evidence: {self.output_dir}", file=sys.stderr)

    def __exit__(self, exc_type, error, traceback):
        try:
            if error is not None:
                if hasattr(error, "cmd"):
                    self.record_command(error.cmd, error)
                self._export(error, traceback)
        except Exception as collection_error:
            print(f"Acceptance evidence collection failed ({type(collection_error).__name__}); preserving original failure", file=sys.stderr)
        finally:
            self.files.clear()
            self.commands = ""
        return False
