"""Build a separate GPL serprog helper from the verified frwr engine; no device I/O."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PINS = {
    "lib/libflashrom.a": "0b72fe14c84f66c2af8a741a827b99fbd0212fc1f1a5eaf7684a74b2a4e4c791",
    "bridge.c": "13049d272d0d48b1b2f26f498f2122cfc12c3615996c008126ef3ba18c137b58",
    "serial-timeout.patch": "b54df87446814426440789a1704d1f83e40a3c9daf721e05fd506f83ec0a4546",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--frwr-root", type=Path, required=True)
    args = parser.parse_args()
    native = args.frwr_root / "gui/native"
    for relative, expected in PINS.items():
        if hashlib.sha256((native / relative).read_bytes()).hexdigest() != expected:
            raise ValueError(f"frwr input checksum mismatch: {relative}")
    output = ROOT / "target/firmware-tools/ms2130-helper"
    output.mkdir(parents=True, exist_ok=True)
    bridge = output / "bridge.c"
    shutil.copyfile(native / "bridge.c", bridge)
    headers = args.frwr_root / "build/tools/flashrom-1.8.0/include"
    gcc = Path("C:/ARM/Toolchain/mingw64/bin/gcc.exe")
    clang = Path("C:/Program Files/LLVM/bin/clang.exe")
    for source in [ROOT / "firmware-tools/ms2130/helper.c", bridge]:
        subprocess.run([str(gcc), "-std=c99", "-O2", "-Wall", "-Wextra", "-Werror", "-D_WIN32_WINNT=0x0601", "-DWINVER=0x0601", "-D__USE_MINGW_ANSI_STDIO=1", "-DIS_WINDOWS=1", "-D__FLASHROM_LITTLE_ENDIAN__=1",
                        "-I", str(headers), "-c", str(source), "-o", str(output / (source.stem + ".o"))], check=True)
    executable = output / "minikmb-ms2130-helper.exe"
    subprocess.run([str(clang), "--target=x86_64-w64-windows-gnu", "--sysroot=C:/ARM/Toolchain/mingw64", "-fuse-ld=lld", "-static", "-municode",
                    "-Wl,--no-insert-timestamp", "-L" + str(native / "runtime"),
                    "-LC:/ARM/Toolchain/mingw64/x86_64-w64-mingw32/lib", "-LC:/ARM/Toolchain/mingw64/lib",
                    "-LC:/ARM/Toolchain/mingw64/lib/gcc/x86_64-w64-mingw32/8.1.0", str(output / "helper.o"), str(output / "bridge.o"),
                    str(native / "lib/libflashrom.a"), "-o", str(executable)], check=True)
    catalog = subprocess.check_output([str(executable), "--catalog"], timeout=10)
    parsed = json.loads(catalog)
    if parsed["event"] != "catalog" or len(parsed["chips"]) < 100:
        raise ValueError("SPI chip database smoke check failed")
    (output / "catalog.json").write_bytes(catalog)
    manifest = {"name":"minikmb-ms2130-helper", "flashrom":"1.8.0", "upstream_sha":"90b6b4376c7480a223e7bb5642e7116315ac6d09",
                "license":"GPL-2.0-or-later", "engine_inputs_sha256":PINS,
                "helper_source_sha256":hashlib.sha256((ROOT / "firmware-tools/ms2130/helper.c").read_bytes()).hexdigest(),
                "executable_sha256":hashlib.sha256(executable.read_bytes()).hexdigest(),"catalog_chips":len(parsed["chips"])}
    (output / "manifest.json").write_text(json.dumps(manifest,indent=2)+"\n",encoding="utf-8")
    print(json.dumps(manifest,indent=2))


if __name__ == "__main__":
    main()
