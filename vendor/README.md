# Vendored upstream sources

Archives preserve upstream source bytes, copyright and license texts. They do not inherit the repository's MIT license. No API keys, local Cargo/npm configuration or node_modules build trees are included.

| Location | Scope | Integrity / provenance |
|---|---|---|
| `rust/` | Exact registry source archives for both `kbm-rs/Cargo.lock` and the standalone SPI Writer GUI lock file, including platform/build/test dependencies; not every locked package is linked into the Windows binary | `rust/manifest.json`: package/version, crates.io URL and Cargo SHA-256 |
| `npm/` | Registry tarballs referenced by Electron renderer/main-process package-lock files, including optional platform packages and build tools | `npm/manifest.json`: package/version, original registry URL, npm SRI and SHA-256 |
| `pico-sdk/` | Pico SDK 2.2.0 core and TinyUSB sources used by RP2040 firmware. Unused Bluetooth/Wi-Fi/lwIP/mbedtls stacks are omitted | Original source revision and archive hash in `pico-sdk/manifest.json` |
| `flashrom/` | Complete corresponding source for the separate GPL SPI helper: flashrom 1.8.0, serprog bridge, Windows timeout patch, frontend and rebuild instructions | Embedded `UPSTREAM-SHA256SUMS`, COPYING.rst and file-level notices |
| `python/` | pyserial 3.5 source for optional host diagnostics | `python/manifest.json` and upstream BSD-3-Clause license in source archive |

STM32 HAL/CMSIS, FreeRTOS, FatFs and the selected TinyUSB device stack are vendored as readable sources under `../firmware/stm32h7/`. TinyUSB is 0.21.0 at `dae3f9a366bfcddbf9dcf1b48d7500286a849539`, with local patches/UPSTREAM notice. FreeRTOS is 10.3.1; HAL source reports 1.11.5. Original notices remain next to these sources.

Fonts and notices are under `../kbm-rs/assets/fonts/`. The Windows runtime ZIP supplies probe-rs 0.32.0, the GPL helper, firmware, source/notice files and a dependency inventory. Compiler toolchains, Electron platform runtime downloads, WSL tools, Windows DLLs and proprietary MCU tools are not presented as project-authored libraries or bundled as source dependencies.

## Using archives

Pico SDK: extract `pico-sdk/pico-sdk-2.2.0-source.tar.gz` into `vendor/pico-sdk/` and set `PICO_SDK_PATH` to its resulting `pico-sdk` directory.

Cargo can use its normal cache or registry resolution with the checked-in lock file. To prepare the standard local Cargo cache from these archives, run `python vendor/scripts/restore-vendor.py --cargo-cache <your Cargo registry cache directory>`. This does not change global Cargo configuration. It copies only verified archives; source extraction/resolution remains Cargo's job. For an air-gapped build, prepare the registry index/cache in advance or use Cargo's standard `cargo vendor` workflow on a connected machine.

For npm, each tarball is a normal registry source package. `npm cache add vendor/npm/<archive>.tgz` can populate a chosen npm cache; the package-lock files remain authoritative. Native binaries/toolchain prerequisites are separate from the library source archives.

Do not assume that a license label alone satisfies every distribution condition. Preserve original notices and the GPL/MPL corresponding source when redistributing their binaries.
