"""Fetch the pinned upstream Windows tool, without installing or connecting a probe."""
from pathlib import Path
import hashlib
import io
import json
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = "0.32.0"
URL = f"https://github.com/probe-rs/probe-rs/releases/download/v{VERSION}/probe-rs-tools-x86_64-pc-windows-msvc.zip"
SHA256 = "56fc0564cc23d604b27dc2d57606194159c49f951999f3e47bd2cbffcba64103"


def fetch(url, limit):
    request = urllib.request.Request(url, headers={"User-Agent": "MiniKMB-build"})
    with urllib.request.urlopen(request, timeout=60) as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError("Upstream tool file exceeds the size limit")
    return data


def main():
    output = ROOT / "target/firmware-tools/probe-rs"
    output.mkdir(parents=True, exist_ok=True)
    if not output.resolve().is_relative_to((ROOT / "target").resolve()):
        raise ValueError("Tool output is outside this workspace target")
    archive = output / "upstream.zip"
    data = archive.read_bytes() if archive.exists() else fetch(URL, 16 * 1024 * 1024)
    if hashlib.sha256(data).hexdigest() != SHA256:
        raise ValueError("Pinned upstream archive checksum mismatch")
    archive.write_bytes(data)
    with zipfile.ZipFile(io.BytesIO(data)) as zipped:
        choices = [item for item in zipped.infolist() if Path(item.filename).name == "probe-rs.exe"]
        if len(choices) != 1 or not 0 < choices[0].file_size <= 100 * 1024 * 1024:
            raise ValueError("Expected one bounded probe-rs.exe")
        executable = zipped.read(choices[0])
    if executable[:2] != b"MZ":
        raise ValueError("Upstream tool is not a Windows executable")
    (output / "probe-rs.exe").write_bytes(executable)
    manifest = {"name": "probe-rs", "version": VERSION, "archive_url": URL,
                "archive_sha256": SHA256, "executable_sha256": hashlib.sha256(executable).hexdigest(),
                "executable_bytes": len(executable), "upstream": "https://github.com/probe-rs/probe-rs",
                "license": "MIT OR Apache-2.0"}
    for license_name in ("LICENSE-MIT", "LICENSE-APACHE"):
        license_file = output / license_name
        if not license_file.exists():
            license_file.write_bytes(fetch(f"https://raw.githubusercontent.com/probe-rs/probe-rs/v{VERSION}/{license_name}", 128 * 1024))
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
