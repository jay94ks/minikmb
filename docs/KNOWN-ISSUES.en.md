# Known limitations and design retrospective

[한국어](KNOWN-ISSUES.md) · [English](KNOWN-ISSUES.en.md)

## Release scope

Current STM32 firmware is v1.0 FS-HID r63. The native app identifies itself as 0.1.0 preview; Electron is 1.0.0. Repository release tags do not change those internal versions. Target USB is implemented and distributed at Full Speed (FS).

## Target USB HS and design retrospective

**The designer's causal assessment:** the omission of **USB-C plug-orientation detection and signal selection**, and the resulting impedance mismatch in this board's HS signal path, is considered the main cause. HS could not be established reliably, or error rates remained high after connection, so stable HS operation was not obtained and the current implementation uses FS.

The designer's retrospective view is that routing the connection through an orientation-aware signal-selection IC would have allowed the HS path to be arranged as intended and its impedance requirements to be met. Orientation detection here means **USB-C plug orientation**, not the ULPI bus DIR signal.

**The schematic was authored directly by the creator, jay94ks.** This is recorded as a design-review omission: proceeding directly to PCB layout without sufficiently checking orientation detection/signal selection and the resulting HS path requirements. It is not classified as GPT incorrectly authoring the schematic. Codex with GPT 6 Astra and Sol assisted with PCB placement/routing, firmware, applications and documentation.

This explanation records the designer's supplied retrospective and causal assessment. Earlier physical tests observed pattern-dependent HS transmit stalls, but a before/after circuit modification or signal-integrity measurement has not yet isolated and confirmed this causal chain. The documentation update is not a new root-cause experiment.

### Distinctions for design review

Orientation-detection logic and an actual data-path switch have different roles. USB 2.0 can also connect the duplicated D+/D− contacts together; the presence of an orientation-detection IC alone is not a universal test of HS capability. The IC's function and placement, PCB stackup, trace geometry, return path and branching must be reviewed together. The assessment above concerns **the designer's evaluation of this board's configuration**. See [TI's USB Type-C design discussion](https://www.ti.com/lit/wp/slyy109b/slyy109b.pdf).

## Other limitations

- The MS2130 USB 3.x capture path is separate from the MCU/USB3300 target path. MS2130 video was confirmed working after installing and programming the external SPI flash.
- Physical boards use 1 MΩ parallel resistors added across Y1/Y2 during bring-up. These are documented rework, not silent changes to the archived manufacturing revision. Review them before another fabrication run.
- Keyboard/mouse behavior depends on the target OS and USB state. Independent interface queues and key-release fixes are included; not every environment is certified.
- HID completion confirms USB transfer completion, not application-level input handling. Key arrays map to physical US HID keys; arbitrary Unicode typing is not provided.
- ISO/UDF support is limited to this project's bootable optical-image workflow, not a complete general-purpose filesystem implementation.
- Long-duration testing and simultaneous two-MCU NCM qualification were outside the required scope.
- Archived Gerbers/BOM reflect the historical partial assembly and DNP SPI flashes. Working MS2130 video required the documented flash/ROM work; review the BOM before another build.
