#!/usr/bin/env python3
"""Prepare Jig's optional, self-hosted OCR assets from verified official sources.

This is a build step, never a document-processing service. It downloads only
fixed engine/model files; it never reads or transmits user documents.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import sys
import tarfile
import tempfile
import urllib.request


ROOT = Path(__file__).resolve().parent.parent
DEST = ROOT / "web" / "vendor" / "ocr"
MANIFEST = DEST / "manifest.json"
MODEL_COMMIT = "923915d4ced2a7235221788285785a29c4a42d4a"
SOURCES = {
    "tesseract.js@6.0.1": (
        "tesseract.js-6.0.1.tgz",
        "https://registry.npmjs.org/tesseract.js/-/tesseract.js-6.0.1.tgz",
        "4a208f2560a527ea9fc339ba2fdf125c13bca6638c194a149379601a1418d0ed",
    ),
    "tesseract.js-core@6.0.0": (
        "tesseract.js-core-6.0.0.tgz",
        "https://registry.npmjs.org/tesseract.js-core/-/tesseract.js-core-6.0.0.tgz",
        "197b53b6bc42b7270c99b079e438992096d71a2b9b8237c6349491bd22fbd5a0",
    ),
    "pdfjs-dist@4.10.38": (
        "pdfjs-dist-4.10.38.tgz",
        "https://registry.npmjs.org/pdfjs-dist/-/pdfjs-dist-4.10.38.tgz",
        "1011b38553532d7078c59f26b15a471f8dae00f101b60e2add9b8511737a1ce0",
    ),
    f"tessdata_fast@{MODEL_COMMIT}": (
        "eng.traineddata",
        f"https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/{MODEL_COMMIT}/eng.traineddata",
        "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2",
    ),
}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def matches(path: Path, entry: dict) -> bool:
    return (
        path.is_file()
        and path.stat().st_size == entry["bytes"]
        and digest(path.read_bytes()) == entry["sha256"]
    )


def download(cache: Path, spec: tuple[str, str, str]) -> Path:
    filename, url, expected = spec
    target = cache / filename
    if target.is_file() and digest(target.read_bytes()) == expected:
        return target
    print(f"Fetching pinned OCR build asset: {filename}", flush=True)
    request = urllib.request.Request(url, headers={"User-Agent": "Jig-OCR-build/1"})
    temporary = target.with_suffix(target.suffix + ".part")
    try:
        with urllib.request.urlopen(request, timeout=90) as response:
            with temporary.open("wb") as output:
                shutil.copyfileobj(response, output)
        actual = digest(temporary.read_bytes())
        if actual != expected:
            raise ValueError(f"SHA-256 mismatch for {filename}: expected {expected}, got {actual}")
        os.replace(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)
    return target


def archive_member(asset: str, source: str) -> str:
    if source == "tesseract.js@6.0.1":
        names = {
            "tesseract.min.js": "package/dist/tesseract.min.js",
            "worker.min.js": "package/dist/worker.min.js",
            "LICENSE-tesseract.js.txt": "package/LICENSE.md",
            "tesseract.min.js.LICENSE.txt": "package/dist/tesseract.min.js.LICENSE.txt",
            "worker.min.js.LICENSE.txt": "package/dist/worker.min.js.LICENSE.txt",
        }
        return names[asset]
    if source == "tesseract.js-core@6.0.0":
        if asset == "LICENSE-tesseract-core.txt":
            return "package/LICENSE"
        if asset in {
            "core/tesseract-core-lstm.wasm.js",
            "core/tesseract-core-simd-lstm.wasm.js",
        }:
            return "package/" + PurePosixPath(asset).name
    if source == "pdfjs-dist@4.10.38":
        names = {
            "pdf/pdf.mjs": "package/build/pdf.min.mjs",
            "pdf/pdf.worker.mjs": "package/build/pdf.worker.min.mjs",
            "LICENSE-pdfjs.txt": "package/LICENSE",
        }
        if asset in names:
            return names[asset]
        if asset.startswith("pdf/cmaps/") or asset.startswith("pdf/standard_fonts/"):
            return "package/" + asset.removeprefix("pdf/")
    raise ValueError(f"Unrecognized asset mapping: {source}: {asset}")


def canonical_gzip(data: bytes) -> bytes:
    compressed = bytearray(gzip.compress(data, compresslevel=9, mtime=0))
    # gzip.compress(mtime=0) uses a platform-specific OS byte on Python 3.11/12.
    # Normalizing it makes the vendored model identical on Linux/macOS/Windows.
    compressed[9] = 255
    return bytes(compressed)


def prepare(cache: Path, entries: list[dict]) -> None:
    cache.mkdir(parents=True, exist_ok=True)
    paths = {source: download(cache, spec) for source, spec in SOURCES.items()}
    archives = {
        source: tarfile.open(path, "r:gz")
        for source, path in paths.items()
        if path.suffix == ".tgz"
    }
    try:
        with tempfile.TemporaryDirectory(prefix="jig-ocr-", dir=cache) as directory:
            staged = Path(directory)
            for entry in entries:
                name, source = entry["path"], entry["source"]
                if name == "LICENSE-tessdata-fast.txt":
                    # This small, versioned license is retained in the repository.
                    data = (DEST / name).read_bytes()
                elif name == "lang/eng.traineddata.gz":
                    data = canonical_gzip(paths[f"tessdata_fast@{MODEL_COMMIT}"].read_bytes())
                else:
                    member = archives[source].getmember(archive_member(name, source))
                    if not member.isfile():
                        raise ValueError(f"Asset is not a regular archive file: {name}")
                    stream = archives[source].extractfile(member)
                    if stream is None:
                        raise ValueError(f"Could not read official package asset: {name}")
                    data = stream.read()
                if len(data) != entry["bytes"] or digest(data) != entry["sha256"]:
                    raise ValueError(f"Generated asset does not match manifest: {name}")
                target = staged / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
            # Verify every output before replacing any existing build assets.
            for entry in entries:
                name = entry["path"]
                target = DEST / name
                target.parent.mkdir(parents=True, exist_ok=True)
                os.replace(staged / name, target)
    finally:
        for archive in archives.values():
            archive.close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=ROOT / ".jig-cache" / "ocr")
    parser.add_argument("--verify-only", action="store_true", help="verify existing assets without network access")
    parser.add_argument("--rebuild", action="store_true", help="regenerate every asset from the verified source cache")
    args = parser.parse_args()
    manifest = json.loads(MANIFEST.read_text())
    entries = manifest["assets"]
    for entry in entries:
        path = PurePosixPath(entry["path"])
        if path.is_absolute() or ".." in path.parts:
            raise ValueError("Unsafe OCR manifest path")
        if entry["source"] not in SOURCES:
            raise ValueError(f"Unrecognized OCR source: {entry['source']}")
    missing = [entry["path"] for entry in entries if not matches(DEST / entry["path"], entry)]
    if args.verify_only and missing:
        raise ValueError("Missing or modified OCR assets: " + ", ".join(missing[:8]))
    if missing or args.rebuild:
        prepare(args.cache.resolve(), entries)
    total = sum(entry["bytes"] for entry in entries)
    print(f"Verified {len(entries)} local OCR assets ({total:,} bytes). No document data was read.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, tarfile.TarError) as error:
        print(f"OCR build assets: {error}", file=sys.stderr)
        raise SystemExit(1)
