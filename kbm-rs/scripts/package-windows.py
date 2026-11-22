"""Build and package a separate, unsigned Windows x64 preview. No device access.

Python 3.11+, pinned Rust/MSVC and cached Cargo packages are build prerequisites.
The resulting application does not run Python/Node or this packaging script.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
from release_support import collect_notices, input_digest, json_bytes, pe_imports, sha, verify_zip, write_zip

ROOT = Path(__file__).resolve().parents[1]


def command(args):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8")


def notices_markdown(inventory):
    lines = ["# MiniKMB Native third-party notices", "",
             "This archive contains the declared licenses and notices from the exact locked Windows normal/build dependency closure; dev-only dependencies are excluded.",
             "Some build dependencies may not be linked into the final executable. License expressions are preserved, not replaced by a general approval label.",
             "The file links below are relative to the extracted ZIP root; individual notice files are assembled during packaging.",
             "Unmodified MPL-2.0 source archives for option-ext and serialport are supplied in sources/ together with their original licenses. The source checksums match Cargo.lock.",
             "Rust standard-library copyright/license notices are in licenses/rust/. Bundled UI font/icon notices are in licenses/ui/ and available in the application settings.",
             "Separate firmware tools are supplied in tools/: probe-rs 0.32.0 (MIT OR Apache-2.0), and minikmb-ms2130-helper (GPL-2.0-or-later). See licenses/firmware-tools/ and the corresponding source/build instructions in sources/minikmb-ms2130-helper-source.tar.gz. Firmware image hashes and tool hashes are in their manifests; firmware is user-provided project payload, not covered by these tool licenses.",
             "This index does not assign a new license to MiniKMB's own source code.", "", "| Package | Declared license | Notices |", "|---|---|---|"]
    for p in inventory["packages"]:
        first = p["notices"][0]["path"]
        lines.append(f"| {p['name']} {p['version']} | {p['license']} | [files]({first}) |")
    return ("\n".join(lines) + "\n").encode("utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--notices-only", action="store_true", help="Generate the offline notice inventory without building or packaging")
    parser.add_argument("--verify", type=Path, help="Read-only ZIP hash and member verification")
    args = parser.parse_args()
    if args.verify:
        manifest = verify_zip(args.verify)
        print(f"PASS {len(manifest['files'])} listed payload files; hash verification is not publisher authentication")
        return
    if sys.platform != "win32":
        raise SystemExit("The release target is Windows x64 MSVC")
    rustup_bin = Path(os.environ.get("USERPROFILE", "")) / ".cargo/bin"
    cargo = str(rustup_bin / "cargo.exe") if (rustup_bin / "cargo.exe").is_file() else shutil.which("cargo")
    rustc = str(rustup_bin / "rustc.exe") if (rustup_bin / "rustc.exe").is_file() else shutil.which("rustc")
    if not cargo or not rustc:
        raise SystemExit("Pinned Rust toolchain is required")
    rust_info = command([rustc, "-vV"])
    pinned = tomllib.loads((ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
    if f"release: {pinned}\n" not in rust_info or "host: x86_64-pc-windows-msvc\n" not in rust_info:
        raise SystemExit("Unexpected Rust version or target")
    subprocess.run([sys.executable, "scripts/ui-assets.py"], cwd=ROOT, check=True)
    if not args.notices_only:
        subprocess.run([sys.executable, "scripts/build-native-keyboard.py"], cwd=ROOT, check=True)
        subprocess.run([cargo, "build", "--release", "--locked", "--offline"], cwd=ROOT, check=True)
    import json
    metadata = json.loads(command([cargo, "metadata", "--format-version", "1", "--locked", "--offline", "--filter-platform", "x86_64-pc-windows-msvc"]))
    rust_sysroot = Path(command([rustc, "--print", "sysroot"]).strip())
    files, inventory = collect_notices(ROOT, metadata, rust_sysroot)
    inventory["rustc"] = rust_info.strip()
    (ROOT / "licenses/windows-dependencies.json").write_bytes(json_bytes(inventory))
    index = notices_markdown(inventory)
    (ROOT / "licenses/THIRD-PARTY.md").write_bytes(index)
    print(f"Collected notices for {len(inventory['packages'])} locked Windows normal/build packages")
    if args.notices_only:
        return
    executable = (ROOT / "target/release/minikmb-native.exe").read_bytes()
    imports = pe_imports(executable)
    if imports["subsystem"] != 2:
        raise ValueError("Expected a Windows GUI release executable")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    readme = (ROOT / "packaging/README-WINDOWS.txt").read_text(encoding="utf-8").replace("@VERSION@",version).encode("utf-8")
    files.update({"minikmb-native.exe": executable, "README.txt": readme, "LICENSE.txt": (ROOT / "LICENSE").read_bytes(),
                  "THIRD-PARTY.md": index, "dependencies.json": json_bytes(inventory), "runtime-imports.json": json_bytes(imports),
                  "rustc-version.txt": rust_info.encode("utf-8"), "Cargo.lock": (ROOT / "Cargo.lock").read_bytes(),
                  "assets-manifest.json": (ROOT / "assets/manifest.json").read_bytes()})
    keyboard = ROOT / "target/native-keyboard"
    keyboard_manifest = json.loads((keyboard / "manifest.json").read_text(encoding="utf-8"))
    keyboard_dll = (keyboard / "minikmb-keyboard.dll").read_bytes()
    keyboard_source = (ROOT / "native-keyboard/keyboard.cpp").read_bytes()
    if sha(keyboard_dll) != keyboard_manifest["sha256"] or sha(keyboard_source) != keyboard_manifest["source_sha256"]:
        raise ValueError("Native keyboard helper/source hash mismatch")
    files.update({"tools/minikmb-keyboard.dll": keyboard_dll,
                  "tools/keyboard-helper.json": json_bytes(keyboard_manifest),
                  "sources/minikmb-keyboard.cpp": keyboard_source,
                  "sources/build-native-keyboard.py": (ROOT / "scripts/build-native-keyboard.py").read_bytes()})
    # Explicit local staging only; missing tools/images fail rather than shipping
    # a downloader or silently dropping the firmware feature.
    stage = ROOT / "target/firmware-tools/package"
    for manifest_name, key, directory in [("tools", "tools", "tools"), ("images", "images", "firmware")]:
        locked = json.loads((ROOT / "firmware-tools" / (manifest_name + ".lock.json")).read_text(encoding="utf-8"))[key]
        for item in locked:
            payload = (stage / directory / item["file"]).read_bytes()
            if sha(payload) != item["sha256"]:
                raise ValueError("Firmware/tool staging hash mismatch: " + item["file"])
            files[directory + "/" + item["file"]] = payload
    for path in sorted(stage.rglob("*")):
        if path.is_file() and path.relative_to(stage).parts[0] not in ["tools", "firmware"]:
            name = path.relative_to(stage).as_posix()
            if name.startswith(("licenses/firmware-tools/", "licenses/firmware/", "sources/")):
                files[name] = path.read_bytes()
    for name in ["tools/manifest.json", "firmware/manifest.json", "sources/minikmb-ms2130-helper-source.tar.gz", "licenses/firmware-tools/FLASHROM-COPYING"]:
        files[name] = (stage / name).read_bytes()
    destination = ROOT / "dist"
    destination.mkdir(exist_ok=True)
    filename = f"MiniKMB-Native-{version}-windows-x64-preview-{sha(executable)[:12]}.zip"
    # The staging file is created in the checked project dist directory only.
    with tempfile.NamedTemporaryFile(dir=destination, prefix=".preview-", suffix=".zip", delete=False) as stage:
        temporary = Path(stage.name)
    try:
        write_zip(temporary, files, {"name": "MiniKMB Native", "version": version, "channel": "preview",
                  "target": "x86_64-pc-windows-msvc", "signed": False, "source_inputs_sha256": input_digest(ROOT),
                  "cargo_lock_sha256": inventory["cargo_lock_sha256"], "executable_sha256": sha(executable)})
        verify_zip(temporary)
        os.replace(temporary, destination / filename)
    finally:
        temporary.unlink(missing_ok=True)
    archive = destination / filename
    checksum = sha(archive.read_bytes())
    (destination / f"{filename}.sha256").write_text(f"{checksum}  {filename}\n", encoding="ascii")
    print(f"PACKAGED {archive}")
    print(f"SHA256 {checksum}")


if __name__ == "__main__":
    main()
