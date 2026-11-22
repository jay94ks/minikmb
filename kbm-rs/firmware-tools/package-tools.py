"""Stage pinned executables, firmware, notices and GPL corresponding source.

All inputs are local; no downloads and no device access. Run this developer step
after fetch/build/stage-images and before package-windows.py.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--frwr-root", required=True, type=Path)
    args = parser.parse_args()
    native = args.frwr_root / "gui/native"
    upstream = args.frwr_root / "build/tools/flashrom-1.8.0"
    output = ROOT / "target/firmware-tools/package"
    sources = {}
    for line in (native / "UPSTREAM-SHA256SUMS").read_text(encoding="utf-8").splitlines():
        checksum, name = line.split("  ", 1)
        if ".." in Path(name).parts or Path(name).is_absolute():
            raise ValueError("Invalid upstream path")
        data = (upstream / name).read_bytes()
        if hashlib.sha256(data).hexdigest() != checksum:
            raise ValueError(f"Upstream source changed: {name}")
        sources["flashrom/" + name] = data
    for name in ["bridge.c", "serial-timeout.patch", "UPSTREAM-SHA256SUMS"]:
        sources[name] = (native / name).read_bytes()
    sources["flashrom-mingw.ini"] = (args.frwr_root / "cmake/flashrom-mingw.ini").read_bytes()
    for name in ["helper.c", "rebuild.py"]:
        sources[name] = (ROOT / "firmware-tools/ms2130" / name).read_bytes()
    sources["README.txt"] = b"MiniKMB MS2130 helper corresponding source (GPL-2.0-or-later)\nflashrom 1.8.0 commit 90b6b4376c7480a223e7bb5642e7116315ac6d09\nChanges: finite Windows serial I/O timeouts; separate serprog frontend, capacity gate before writes, preserve tail, full readback compare.\nSee flashrom/COPYING.rst and source file notices. Run python3 rebuild.py under Linux/WSL with Meson, Ninja, patch and mingw-w64 GCC installed. Compiler versions can change output hashes.\n"
    blob = io.BytesIO()
    with tarfile.open(fileobj=blob, mode="w:gz") as archive:
        for name, data in sorted(sources.items()):
            info = tarfile.TarInfo("minikmb-ms2130-helper/" + name)
            info.size = len(data)
            info.mode = 0o644
            info.mtime = 0
            archive.addfile(info, io.BytesIO(data))
    files = {"sources/minikmb-ms2130-helper-source.tar.gz": blob.getvalue(),
             "licenses/firmware-tools/FLASHROM-COPYING": (upstream / "COPYING.rst").read_bytes(),
             "licenses/firmware-tools/GCC-COPYRIGHT.txt": (native / "runtime/GCC-COPYRIGHT.txt").read_bytes(),
             "tools/manifest.json": (ROOT / "firmware-tools/tools.lock.json").read_bytes()}
    for name in ["LICENSE-MIT", "LICENSE-APACHE"]:
        files["licenses/firmware-tools/probe-rs-" + name] = (ROOT / "target/firmware-tools/probe-rs" / name).read_bytes()
    files["licenses/firmware-tools/README.txt"] = b"probe-rs 0.32.0, MIT OR Apache-2.0: https://github.com/probe-rs/probe-rs/releases/tag/v0.32.0\nminikmb-ms2130-helper: GPL-2.0-or-later; corresponding source is supplied in sources/minikmb-ms2130-helper-source.tar.gz. GCC runtime notices and exception are included separately.\nThese are separate executables invoked by MiniKMB; no runtime downloader is included.\nFirmware images are user-supplied project payloads, not licensed by these tool licenses.\n"
    for name, data in files.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    # Hash validation happens before the release-stage writes.
    subprocess.run([sys.executable, str(ROOT / "firmware-tools/stage-runtime.py"), "--directory", str(ROOT / "target/release")], check=True)
    for folder in ["tools", "firmware"]:
        for path in (ROOT / "target/release" / folder).iterdir():
            if path.is_file():
                destination = output / folder / path.name
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(path.read_bytes())
    print(json.dumps({"source_files": len(sources), "payload_files": len(files), "ready": True}))


if __name__ == "__main__":
    main()
