"""Build the r63 source list with GNU Arm Embedded; never flash a device."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--toolchain-bin", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "build" / "portable")
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)

    def tool(name):
        command = "arm-none-eabi-" + name + (".exe" if os.name == "nt" else "")
        found = args.toolchain_bin / command if args.toolchain_bin else shutil.which(command)
        if not found or not Path(found).is_file():
            raise SystemExit("Missing " + command + "; use --toolchain-bin")
        return str(found)

    plan = json.loads((ROOT / "tools/build-plan.json").read_text())
    gcc, gpp, objcopy = tool("gcc"), tool("g++"), tool("objcopy")

    def compile_unit(unit):
        source = unit["source"]
        obj = output / (source + ".o")
        obj.parent.mkdir(parents=True, exist_ok=True)
        result = subprocess.run(
            [gcc, *unit["flags"], source, "-o", str(obj)],
            cwd=ROOT, capture_output=True, text=True,
        )
        if result.returncode:
            raise RuntimeError(source + "\n" + result.stdout + result.stderr)
        if result.stderr:
            print(result.stderr, end="")
        return obj

    with ThreadPoolExecutor(max_workers=max(1, min(args.jobs, 16))) as pool:
        objects = list(pool.map(compile_unit, plan["units"]))
    elf = output / "MiniKMB-FS-HID.elf"
    response = output / "objects.list"
    response.write_text("\n".join('"' + str(p).replace("\\", "/") + '"' for p in objects))
    subprocess.run([
        gpp, "@" + str(response), "-o", str(elf), "-mcpu=cortex-m7",
        "-T" + str(ROOT / "STM32H743VIHX_FLASH.ld"), "--specs=nosys.specs",
        "-Wl,-Map=" + str(output / "MiniKMB-FS-HID.map"), "-Wl,--gc-sections",
        "-static", "--specs=nano.specs", "-mfpu=fpv5-d16", "-mfloat-abi=hard",
        "-mthumb", "-Wl,--start-group", "-lc", "-lm", "-lstdc++", "-lsupc++",
        "-Wl,--end-group",
    ], cwd=ROOT, check=True)
    binary = elf.with_suffix(".bin")
    subprocess.run([objcopy, "-O", "binary", str(elf), str(binary)], check=True)
    subprocess.run([tool("size"), str(elf)], check=True)
    print(json.dumps({"units": len(objects), "bin_bytes": binary.stat().st_size,
                      "bin_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}))


if __name__ == "__main__":
    main()
