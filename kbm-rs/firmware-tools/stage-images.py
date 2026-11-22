"""Copy explicitly provided firmware to the package staging area after hash checks."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stm32", required=True, type=Path)
    parser.add_argument("--ms2130", required=True, type=Path)
    args = parser.parse_args()
    manifest = json.loads((ROOT / "firmware-tools/images.lock.json").read_text(encoding="utf-8"))
    staged = []
    for image in manifest["images"]:
        source = getattr(args, image["target"])
        data = source.read_bytes()
        if len(data) != image["bytes"] or hashlib.sha256(data).hexdigest() != image["sha256"]:
            raise ValueError(f"{image['target']}: firmware size/hash differs from the pinned image")
        staged.append((image, data))
    output = ROOT / "target/firmware-tools/package/firmware"
    output.mkdir(parents=True, exist_ok=True)
    for image, data in staged:
        (output / image["file"]).write_bytes(data)
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"staged": [{"id": i["id"], "bytes": len(d), "sha256": i["sha256"]} for i, d in staged]}))


if __name__ == "__main__":
    main()
