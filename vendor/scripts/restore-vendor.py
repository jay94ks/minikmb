"""Copy verified crate archives into an explicitly selected Cargo cache."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-cache", type=Path, required=True,
                        help="Registry-specific cache directory, e.g. ~/.cargo/registry/cache/index.crates.io-...")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1] / "rust"
    entries = json.loads((root / "manifest.json").read_text())
    for item in entries:
        name = item["archive"]
        if Path(name).name != name or not name.endswith(".crate"):
            raise ValueError("Invalid archive basename")
        data = (root / name).read_bytes()
        if hashlib.sha256(data).hexdigest() != item["sha256"]:
            raise ValueError("Checksum mismatch: " + name)
    args.cargo_cache.mkdir(parents=True, exist_ok=True)
    for item in entries:
        shutil.copyfile(root / item["archive"], args.cargo_cache / item["archive"])
    print("Copied", len(entries), "verified archives; registry index/configuration unchanged")


if __name__ == "__main__":
    main()
