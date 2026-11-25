# Building MiniKMB

[한국어](BUILDING.md) · [English](BUILDING.en.md)

Build tools never flash a board automatically. Keep runtime settings/API keys outside the checkout. The source archive intentionally excludes caches, device logs and private development journals.

## Native Windows app

Use Windows x64, MSVC/Windows SDK, Python 3.11+ and the pinned `kbm-rs/rust-toolchain.toml` (Rust 1.95.0). From `kbm-rs/`:

```powershell
$env:CARGO_INCREMENTAL = '0'
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python scripts/ui-assets.py
python scripts/build-native-keyboard.py
cargo build --release --locked --bins
```

`ui-assets.py` verifies the bundled fonts/licenses offline unless explicitly asked to fetch. The Rust unit tests use neighboring Electron keyboard mapping source; keep the repository layout intact. The keyboard DLL must be staged with the executable for special-key capture.

### Local CLI

The same build produces `target/release/minikmb-native.exe` and `minikmb-cli.exe`. Start the updated GUI, then run `minikmb-cli.exe channels`. TCP REST/MCP and API keys are not required. Tables/text are the default; use `--json` for JSON. See the [CLI guide](../kbm-rs/docs/CLI.en.md).

### Tools, firmware and ZIP

The shipped app ZIP already contains the tools and firmware; it does not download executables at runtime. Developers may use `firmware-tools/fetch-probe-rs.py` to fetch the pinned open-source build tool, or extract the verified tool payload from a published ZIP. `firmware-tools/tools.lock.json` and `images.lock.json` pin the exact bytes.

The GPL helper's complete source/rebuild instructions are in `vendor/flashrom/minikmb-ms2130-helper-source.tar.gz`. The developer staging flow is `stage-images.py` → `package-tools.py` → `scripts/package-windows.py`. `package-tools.py --frwr-root <writer>` expects the writer/flashrom build inputs described in that source archive. For packaging without rebuilding those external tools, extract the release ZIP's tools, firmware, licenses and sources into `kbm-rs/target/firmware-tools/package/`, preserving their directory layout and manifests. For development execution, also copy the ZIP's tools/firmware directories next to the application in `target/release/`.

Run `python scripts/package-windows.py`, then `python scripts/package-windows.py --verify <zip>`. The packager verifies pinned binaries/notices and includes MIT, GPL/MPL source/notices and dependency manifests. A new executable should be launched and tested before making a new distributable ZIP. The published ZIP uses the executable already confirmed during hands-on testing.

## Electron v1.0

From `kbm/` run `npm ci`, `npm ci --prefix src-electron`, `npm run typecheck`, then `npm run build`. Node must satisfy the version in package.json. Windows/MSVC is required for the native keyboard helper; the prebuild script is `../work/build-minikmb-native.cjs`.

Both lock files and registry source archives are preserved. Electron's platform runtime is installed by its normal package install process; the source vendor directory is not a replacement for that runtime or Windows toolchain.

## RP2040 master/slave

Use WSL with CMake, Ninja and GNU Arm Embedded. Extract the archived Pico SDK 2.2.0 (see `vendor/README.md`) or use that exact SDK revision. From the repository root in WSL:

```sh
export PICO_SDK_PATH=/absolute/path/to/pico-sdk
cmake -S master -B master/build-wsl -G Ninja -DCMAKE_BUILD_TYPE=Release -DPICO_SDK_PATH="$PICO_SDK_PATH"
cmake --build master/build-wsl
cmake -S slave/kbm-slave -B slave/kbm-slave/build-wsl -G Ninja -DCMAKE_BUILD_TYPE=Release -DPICO_SDK_PATH="$PICO_SDK_PATH"
cmake --build slave/kbm-slave/build-wsl
```

With picotool available, Pico SDK can generate UF2. `-DPICO_NO_PICOTOOL=1` builds ELF/BIN without fetching picotool; convert explicitly with `picotool uf2 convert --quiet <elf> <uf2> --family rp2040`. No flashing is performed. The PowerShell `build-wsl.ps1` wrappers use the same process and take `PICO_SDK_PATH` as a Windows environment path.

## STM32H7

Import `firmware/stm32h7/` as an existing STM32CubeIDE project, use GNU Tools for STM32 13.3.rel1 and build the Debug configuration. The `.ioc`, linker scripts and all required HAL/middleware source subsets are included.

A portable build is also available:

```sh
python firmware/stm32h7/tools/build.py --toolchain-bin /path/to/arm-gnu/bin
```

The explicit source/flag plan was exported from the r63 configuration. It builds 93 translation units into `firmware/stm32h7/build/portable/`, emits ELF/BIN/MAP, reports size and SHA-256, and never contacts a device. Debug paths and compiler versions can change artifact hashes; this is not a claim of bit-for-bit reproducibility with the archived, hardware-tested ELF. The release retains that exact r63 ELF/BIN and hashes.

Raw STM32 BIN programming starts at `0x08000000`; ELF carries its own addresses. The target part is STM32H743VIH6. Before physical flashing, select the actual programmer and intended board; do not flash every same-VID/PID device.

## RP2040 SPI Writer

Use `firmware/flash-writer/` with Pico SDK 2.2.0 and the pins in its README. Build its CMake project to produce `frwr.uf2`. The `gui/` source preserves the standalone flashrom-based UI; the integrated MiniKMB app uses a separate GPL helper. Preserve corresponding source and notices for the binaries you distribute.
