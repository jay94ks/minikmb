"""Place already downloaded/built tools and images next to a development exe.

No network, installer, probe access or flashing. Product packaging uses the same
hash-pinned inputs; this script is a developer build step, never an app downloader.
"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def checked(source, expected):
    data = source.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError(f"Staging input hash mismatch: {source.name}")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", required=True, type=Path)
    args = parser.parse_args()
    if not args.directory.is_absolute() or not args.directory.resolve().is_relative_to((ROOT / "target").resolve()):
        raise ValueError("Development staging must stay inside this workspace target")
    manifest = json.loads((ROOT / "firmware-tools/tools.lock.json").read_text(encoding="utf-8"))
    candidates = {"swd": "probe-rs/probe-rs.exe", "spi": "ms2130-helper/minikmb-ms2130-helper.exe"}
    files = {}
    for tool in manifest["tools"]:
        files["tools/" + tool["file"]] = checked(ROOT / "target/firmware-tools" / candidates[tool["id"]], tool["sha256"])
    images = json.loads((ROOT / "firmware-tools/images.lock.json").read_text(encoding="utf-8"))
    for image in images["images"]:
        files["firmware/" + image["file"]] = checked(ROOT / "target/firmware-tools/package/firmware" / image["file"], image["sha256"])
    files["firmware/manifest.json"] = json.dumps(images, indent=2).encode("utf-8")
    for name, data in files.items():
        destination = args.directory / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    print(f"Staged {len(files)} verified tool/firmware files")


if __name__ == "__main__":
    main()
