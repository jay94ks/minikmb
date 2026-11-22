# License scope and attribution

[한국어](NOTICE.ko.md) · [English](NOTICE.md)

Copyright (c) 2026 jay94ks. MiniKMB-authored software, hardware design files and documentation are MIT licensed unless an individual file carries another license or contains third-party material. Development used OpenAI Codex with GPT 6 Astra and Sol. The schematic was authored directly by jay94ks; the target USB-C HS design-review omission is the creator's retrospective assessment and is not attributed to GPT incorrectly authoring the schematic.

The root MIT license does not replace third-party licenses, grant rights to third-party trademarks, or relicense manufacturer documentation and firmware blobs.

- `firmware/stm32h7/Drivers/CMSIS`: Apache-2.0 and the ST device-package license; preserve each LICENSE and file header.
- `firmware/stm32h7/Drivers/STM32H7xx_HAL_Driver`: its original package notice (BSD-3-Clause fallback) applies.
- `firmware/stm32h7/Middlewares/Third_Party/TinyUSB`: MIT, upstream origin and local patches are in `UPSTREAM.md`.
- FreeRTOS: MIT. FatFs: original ChaN permission/copyright notice in the source files. Local changes are distributed with these notices intact.
- `vendor/pico-sdk`: Pico SDK and bundled library sources retain all original file-level licenses, including BSD-3-Clause and TinyUSB MIT.
- `vendor/rust`, `vendor/npm`: unmodified registry source archives; inspect each package's original license and notices. Lock checksums and source URLs are recorded in manifests. The native Windows distribution also supplies collected notice text and MPL source archives.
- `vendor/flashrom`: corresponding source for the separately executed GPL-2.0-or-later SPI helper. Its COPYING and file-level notices control these files, including any stricter GPL-only source files. GPL code is not relicensed MIT. The flash-writer GUI links libflashrom; combined distributions must comply with the applicable GPL terms.
- probe-rs 0.32.0: MIT OR Apache-2.0, separately invoked; notices are supplied with the release. No proprietary ST-Link CLI is redistributed.
- Noto Sans KR: OFL-1.1; Lucide font/icons: ISC. Notices are next to the font files.
- KiCad standard-library-derived symbols/footprints: original KiCad library terms, including CC-BY-SA-4.0 and its exception where applicable. Supplier-derived geometry and pin data retain attribution to their suppliers. See CAD library notices.
- `references/` PDFs: complete, unmodified manufacturer/publisher documents, copyright retained; root MIT does not apply. They are references, not project-authored specifications. Availability or inclusion does not grant additional rights.
- `MS2130_sample_20221026.bin`: owner-supplied MS2130 firmware used on the physical project, 64,468 bytes, SHA-256 `32c729ced368413fc16ff000bb2a044b6991aa5f3d04fa6cfd63056f40779538`. No separate public source license or redistribution license was supplied with this blob. It is not MIT or claimed to be open-source. Rights remain with its original rights holder; independent reuse must be checked with that holder.

Development photographs were supplied by the project owner for this public repository. Incidental third-party products, logos and software shown remain the property of their respective owners. No endorsement is implied.
