# Firmware programming

The app provides standalone programmer operations for new boards. No active channel or pre-registered MCU UID is required. Select the programmer and input image explicitly; no auto-flash occurs on connection.

- STM32H743VIH6: invoke bundled open-source probe-rs 0.32.0 through ST-Link/SWD. ELF addresses are preserved; raw BIN starts at 0x08000000. Verify the programmed result; do not apply full-chip mass erase as a substitute for the intended image operation.
- MS2130 external SPI flash: invoke the separate GPL serprog helper through the RP2040 Writer. Capacity is checked before erase; undersized chips are rejected. The surrounding tail is preserved and full readback verified. Manual chip selection is not required in the app; internal chip/driver identification still protects the write path.
- Current bundled image: STM32 FS-HID r63 (v1.0.0, features0x77). The target uses Full Speed; see repository KNOWN-ISSUES.md for the unresolved physical HS transmit issue.
- MS2130 ROM: owner-supplied 2022-10-26 image, 64,468 bytes. Its rights are separate from the MIT application; see repository NOTICE.md.

Runtime binaries, image sizes/hashes and licenses are pinned by firmware-tools/*.lock.json. The release ZIP contains the tools, not a runtime downloader. GPL helper corresponding source accompanies its executable. Programming keeps power stable and reports completion/readback distinctly from a successful build.
