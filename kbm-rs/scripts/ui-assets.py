"""Verify pinned UI assets offline, or restore them with explicit --fetch.

No package install, archive extraction, system font access, or runtime fetch.
"""
import argparse
import base64
import hashlib
import io
import json
from pathlib import Path
import re
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
FONTS = ROOT / "assets/fonts"
MANIFEST = ROOT / "assets/manifest.json"


def download(url):
    request = urllib.request.Request(url, headers={"User-Agent": "MiniKMB-asset-preparation/0.1"})
    with urllib.request.urlopen(request, timeout=45) as response:
        data = response.read(32 * 1024 * 1024 + 1)
    if len(data) > 32 * 1024 * 1024:
        raise ValueError("Asset download exceeds bound")
    return data


def checked(name, data, digest):
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError(f"Pinned SHA256 mismatch: {name}")
    return data


def fetch(manifest):
    noto, lucide = manifest["noto"], manifest["lucide"]
    files = {
        noto["file"]: checked(noto["file"], download(noto["url"]), noto["sha256"]),
        noto["license_file"]: checked(noto["license_file"], download(noto["license_url"]), noto["license_sha256"]),
    }
    notice = ("\u00a9 2014-2021 Adobe (http://www.adobe.com/).\n\n"
              "Unmodified Noto Sans KR Regular, version 2.004.\n"
              "SIL Open Font License 1.1: Noto-OFL.txt\n").encode("utf-8")
    files[noto["notice_file"]] = checked(noto["notice_file"],notice,noto["notice_sha256"])
    blob = download(lucide["url"])
    integrity = "sha512-" + base64.b64encode(hashlib.sha512(blob).digest()).decode("ascii")
    if integrity != lucide["npm_integrity"]:
        raise ValueError("Pinned npm distribution integrity mismatch")
    with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as archive:
        for member, name, digest in [
            ("package/font/lucide.ttf", lucide["file"], lucide["sha256"]),
            ("package/LICENSE", lucide["license_file"], lucide["license_sha256"]),
        ]:
            stream = archive.extractfile(member)
            if stream is None:
                raise ValueError(f"Missing official package member: {member}")
            data = stream.read(2 * 1024 * 1024 + 1)
            if len(data) > 2 * 1024 * 1024:
                raise ValueError("Font or license exceeds bound")
            files[name] = checked(name, data, digest)
        css_stream = archive.extractfile("package/font/lucide.css")
        if css_stream is None:
            raise ValueError("Missing official glyph map")
        css = css_stream.read(2 * 1024 * 1024 + 1).decode("utf-8")
        for name, codepoint in lucide["glyphs"].items():
            match = re.search(r'\.icon-' + re.escape(name) + r'::before \{ content: "\\([a-f0-9]+)";', css)
            if not match or int(match[1], 16) != codepoint:
                raise ValueError(f"Pinned Lucide glyph mismatch: {name}")
    # Validate all downloads first. Only the five fixed project asset files can
    # be written; archive members are read in memory and never extracted.
    for name, data in files.items():
        if Path(name).name != name:
            raise ValueError("Expected a fixed asset basename")
        (FONTS / name).write_bytes(data)


def verify(manifest):
    for asset in manifest.values():
        font = (FONTS / asset["file"]).read_bytes()
        checked(asset["file"], font, asset["sha256"])
        if len(font) != asset["bytes"]:
            raise ValueError("Pinned asset length mismatch")
        checked(asset["license_file"], (FONTS / asset["license_file"]).read_bytes(), asset["license_sha256"])
        if "notice_file" in asset:
            checked(asset["notice_file"], (FONTS / asset["notice_file"]).read_bytes(), asset["notice_sha256"])
    print("PASS pinned font bytes, versions, licenses and notice hashes (offline)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fetch", action="store_true", help="Restore from the fixed official URLs; network required")
    args = parser.parse_args()
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if args.fetch:
        fetch(manifest)
    verify(manifest)


if __name__ == "__main__":
    main()
