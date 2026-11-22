# Tests and release validation

Normal tests are host-side and do not send HID input, format SD media, reset boards or change host NIC configuration. Hardware tests remain explicit/ignored by default. Do not treat mock tests as a physical signal-integrity or target-OS result.

## Rust desktop

From `kbm-rs/`, use the pinned Windows toolchain:

```powershell
$env:CARGO_INCREMENTAL = '0'
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python -m unittest discover -s scripts -p test_release_support.py
```

The publication checkout passed 268 library tests and 15 binary/UI tests (283 total); five hardware tests were ignored. The packaging fixture suite passed eight tests. The final app archive was checked against its manifest and the hands-on-approved executable SHA-256.

## RP2040 firmware

From repository root in WSL, set PICO_SDK_PATH:

```sh
cmake -S common/tests -B build/host-tests -DCMAKE_BUILD_TYPE=Debug -DPICO_SDK_PATH="$PICO_SDK_PATH"
cmake --build build/host-tests --parallel
ctest --test-dir build/host-tests --output-on-failure
```

Nine host suites passed, covering protocols, queues, routing, DMA cursor accounting, wake, HID controls and boot sessions. Master and slave were also rebuilt from the publication checkout to generate fresh UF2 assets. These steps do not flash the boards.

## STM32 firmware

`tools/build.py` completed the 93-source ARM build. Host regression fixtures can be run in WSL/Linux:

```sh
python3 firmware/stm32h7/tools/test_target_hid_usb.py
python3 firmware/stm32h7/tools/test_management_hid.py
```

FS/HS adapter configurations, independent keyboard/mouse scheduling and management-HID wire fixtures passed. The HS fixtures simulate endpoints; they do not claim that the physical High Speed transmit stall has been repaired.

Additional C/Python storage, optical-media and protocol tests are retained under `firmware/stm32h7/tools/`. Inspect a script before running it; `probe_*`/upload tools are for explicit device diagnostics, not unit tests.

## Electron

Run `npm run typecheck` and `npm run build` under `kbm/`. Pure regression scripts are in `work/*-tests.cjs` and use mocked transports. They expect `kbm/node_modules` and the repository layout.

## Physical scope

The release executable was launched and the channel-card drag order was confirmed before packaging. Other feature tests were performed during development, with unresolved limitations summarized in KNOWN-ISSUES.md. Long-duration testing, simultaneous two-MCU NCM qualification and blanket target OS/cable/hub certification are not claimed.
