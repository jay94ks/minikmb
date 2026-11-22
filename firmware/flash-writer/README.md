# MS2130 SPI ROM Writer

This RP2040/Pico serprog SPI NOR writer was made during STM32H7-board bring-up to program the external flash used by the MS2130 capture ICs. It is not the older three-RP2040 KVM bridge.

| Signal | Pico GPIO |
|---|---|
| SCK / MOSI / MISO / CSn | GP2 / GP3 / GP4 / GP5 |
| WPn / HOLDn | GP6 / GP7 |
| Write / read / USB LED | GP14 / GP15 / GP25 |

Use 3.3 V-compatible wiring and the selected chip's pinout. The programmer's own boot flash is separate from the external flash. Capacity must be at least the input image size; the host tool refuses undersized parts before erase and verifies full readback. Unwritten tail bytes are preserved by the application workflow.

Build with Pico SDK 2.2.0: `cmake -S . -B build -G Ninja -DPICO_SDK_PATH=<sdk> -DPICO_BOARD=pico`, then `cmake --build build`. The SDK source and TinyUSB revision are recorded in `../../vendor/pico-sdk/manifest.json`.

The native MiniKMB app invokes a separate GPL helper executable. Its exact corresponding source is `../../vendor/flashrom/minikmb-ms2130-helper-source.tar.gz`, including build instructions, flashrom sources, Windows serial-timeout patch and license. The older standalone Rust/egui GUI source is preserved in `gui/`; it links libflashrom and must be distributed under the applicable GPL conditions, rather than treating the combined executable as MIT-only.

The MS2130 ROM is an externally supplied firmware blob; its rights are separate from the writer. See the repository NOTICE and release notices.
