# Current release scope

- Current STM32 firmware is v1.0 FS-HID r63. The native PC app identifies itself as 0.1.0 preview; Electron is 1.0.0. Repository release tags do not silently change these internal versions.
- Target USB uses Full Speed. USB3300/ULPI High Speed transfers have reproduced pattern-dependent transmit stalls. Firmware, bus capture and timing investigations have not established a confirmed root cause. Do not claim the High Speed issue is fixed or definitively assign it to PCB impedance, NXT timing, DMA/cache, or the crystal.
- This MCU target path is distinct from the MS2130 USB 3.x capture path. MS2130 video was confirmed working after installing and programming the external SPI flash.
- Physical boards used 1 MΩ parallel resistors added across Y1/Y2 during bring-up. These are documented rework, not silently included in the archived manufacturing revision. Review before making another batch.
- Keyboard/mouse interaction depends on the target OS and USB state. The release contains independent keyboard/mouse queues and the app's keyboard-release fixes. Tests do not certify every OS, hub, cable or keyboard layout.
- USB HID completion confirms transfer completion, not that the target application acted on the input. Key-array strings map to physical US HID keys; arbitrary Unicode typing is not provided.
- ISO/UDF support is intentionally limited to the project's bootable optical-image workflow. It is not a general-purpose ISO/UDF filesystem implementation.
- Long-duration operation and simultaneous NCM operation on both MCUs were outside the required test scope. No broad reliability certification is claimed.
- The archived Gerbers/BOM reflect the historical partially assembled order, including DNP SPI flashes. A functioning MS2130 needs the documented flash/ROM work. Review BOM before repeating fabrication.
