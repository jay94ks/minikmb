# v1.0 release guide

[한국어](RELEASE-1.0.md) · [English](RELEASE-1.0.en.md)

The [GitHub Release](https://github.com/jay94ks/minikmb/releases/tag/v1.0.0) provides the Windows app, firmware, ROM, notices and checksums. Internal versions are native app 0.1.0 preview and STM32 firmware 1.0.0 FS-HID r63.

| File | Purpose |
|---|---|
| `MiniKMB-Native-0.1.0-windows-x64-preview-267a4d46464d.zip` | App, programming tools, firmware, notices and corresponding source |
| `MiniKMB-FS-HID-r63.elf` / `.bin` | Current STM32H743VIH6 firmware; raw BIN starts at 0x08000000 |
| `kbm-master.uf2` / `kbm-slave.uf2` | RP2040 KVM management/target firmware |
| `frwr-0.1.0.uf2` | External SPI ROM Writer used for the H7 board's MS2130 |
| `MS2130_sample_20221026.bin` | 64,468-byte image for MS2130 external flash |
| `firmware-notices.zip`, `LICENSE.txt`, `NOTICE.md` | Project and third-party notices |
| `release-manifest.json`, `SHA256SUMS` | Build provenance, sizes and SHA-256 |

The hands-on-confirmed app executable has SHA-256 `267a4d46464d1cb675d351905a6ce24eb296202c8462bd8f7124c68668f01d85`. This bilingual documentation update does not replace the published binaries.

## HS and design retrospective

Target USB is implemented at FS. The designer attributes unreliable HS establishment, or high error rates after connection, to the omitted USB-C plug-orientation detection/signal-selection circuit and the resulting impedance mismatch in this board's HS path.

The schematic was authored directly by jay94ks. Proceeding to PCB layout before sufficiently reviewing this portion is recorded as a design-review omission, not GPT incorrectly authoring the schematic. This is the designer's retrospective assessment, distinct from a root cause confirmed by isolation measurements. See [known limitations](KNOWN-ISSUES.en.md).

MS2130 USB 3.x video is separate from the MCU/USB3300 target path. Video was confirmed working after fitting and programming the external SPI flash.

MiniKMB-authored code, designs and documentation are MIT. Third-party libraries/tools and the MS2130 ROM retain separate rights. No public redistribution license was established for the ROM; see [NOTICE](../NOTICE.md) and the original license terms.
