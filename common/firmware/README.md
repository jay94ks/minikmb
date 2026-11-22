# Firmware architecture and injected backends

## USB wake injection

`usb_wake.hpp` accepts injected USB and microsecond-clock backends. Explicit
results distinguish sent versus ignored (active, host disabled, disconnected,
USB transition, unsupported). Wait at least5ms after suspend; never synthesize
HID or retry an unsupported automatic wake in the same suspend. Slave's static
epoch mailbox defers the UART response to USB core, bounded by100ms. No TinyUSB
calls occur on UART core. A sent signal cannot prove PC wake or S5 support.

## UART DMA transport

`common/uart_dma_transport.cpp` supplies a C ABI backed by static C++ contexts.
Master claims six DMA channels (RX ping-pong +TX per UART), slave claims three.
RX captures16-bit UART DR words, preserving error flags, into two aligned2KiB
halves (2048 characters total). Hardware ring wrapping and chained transfer-count
reload keep capture continuous; IRQ only records half completions, never parses
packets or blocks on queues. Foreground reads the current DMA progress, so even
one byte of a partial half reaches the progressive decoder immediately.

`DmaRxCursor<Capacity>` injects the progress/word backend for WSL mock tests.
Overflow, raced overwrite, UART error flags or coalesced completion IRQs invalidate
the decoder rather than accepting a frame spanning lost bytes. RX is discarded
on baud changes. TX DMA reads an immutable encoded variable-length frame; the
buffer may be reused only after DMA completes. Baud/reboot also wait for UART
BUSY (DMA completion alone does not mean the last stop bit left the pin).
Master registers completed TX before draining buffered ACKs. Wire v3, queue
priorities,921600 baseline, and48KiB sector cache are unchanged.


## v3 bounded frames and static queues

Common C ABI codecs live in frame_protocol.h/.inc:16-bitLE LEN, maximum4118
decoded bytes, checksum includes both LEN bytes. Compact packets remain255-byte
payloads. Large FS uses explicitly supplied decoder buffers and distinct1-entry
transfer queues, never4KiB elements multiplied by every64-entry control queue.
common/static_queue.h supplies capacity+1 static storage to Pico queue operations,
including normal/serial/network/native-NCM queues. No packet queue calloc/free.
Slave data cache is48KiB,96 independently tagged512-byte sectors, separate CRC
staging4KiB. Requests cover1..8sectors as needed, no fixed4KiB padding. Larger
FS frame arbitration is non-preemptible while on UART:worst-case escaped4KiB
is about89.4ms at921600. Ethernet retains its64-byte fragments.

Master/slave control logic uses C++20; C ABI packet codecs and TinyUSB/Pico glue
remain C. These are deliberate boundaries, not a wholesale filename conversion.
No RTTI, exceptions, virtual dispatch or hot-path allocation is introduced.
Pico queue storage is statically reserved with capacity+1 ring entries; no packet queue calloc/free.

| Layer | Portable implementation | RP2040 backend | Host mock |
|---|---|---|---|
| Mode / USB visibility | `NativeModeController`, `NcmVisibility` | `pico::Clock`, `pico::UsbNcm<Descriptors>`, port publisher | manual clock, recorded disconnect/connect, fake ports |
| UART byte budgets | `UartIo<Driver>` | `pico::Uart` direct FIFO access | partial RX reads / limited TX slots |
| Bounded queues | `Queue<T, Capacity, Backend>` | Pico multicore-safe queue view | fixed-size typed storage |
| Per-port NCM lifecycle | `NativeUartSession` | UART scheduler + real response tracker | deterministic request/ACK/timeout sequence |
| Ethernet TX | `FragmentSender` | same class in master and slave | 1514-byte roundtrip / CRC / credits |
| USB→UART Ethernet selection | `EthernetRouter<2>` | runtime online/ready bitmask | MAC learning / broadcast / offline masks |
| DHCP wire codec | `native_dhcp.c` (C ABI) | master's core 0 | identical parser/lease engine on host |
| Read-only virtual disk | `DiskReader` | core-1 MSC callbacks + bounded queues | sector fragments, CRC, heartbeat/timeout/eject |

## Backend contracts

Clock: `uint32_t now()` in milliseconds. Deadline comparisons are wrap-safe for
intervals under 2^31 ms. USB: `visible`, `setVisible`, `disconnect`, `connect`,
`ready`. Descriptors only change while detached; reconnect after 300 ms.
`NativeModeController` waits for the actual CDC response write plus 200 ms before
publishing a mode change. Its port publisher owns cross-core atomic publication.

Queue backend: `push(const void*, bytes, capacity)`, `pop(void*, bytes)`, `size()`.
The backend must match T/capacity and provide required multicore synchronization.
The template itself never allocates and does not pretend a full/empty check is
an atomic reservation. Multi-destination enqueue requires one producer and a
preflight capacity check (as in native USB RX). Do not substitute the single-
threaded test backend for production's multicore queue.

UART: `readable`, `writable`, `read`, `write`. RX budget 64 bytes and TX budget
32 bytes allow the other UART and USB work to progress. Frame transmission is
never interleaved. `NativeUartSession` accepts one request at a time and advances
only on a matching real slave ACK; the outer response tracker establishes a
PING ordering barrier after timeout. HID/control runs before network fragments.

## Ownership and optimization

Master core 0 owns TinyUSB/descriptors, native mode state, MAC learning and DHCP.
Core 1 owns both UART drivers, independent session objects and response trackers.
Slave core 0 owns UART/validation; core 1 owns TinyUSB/HID, visibility and serial/
network callbacks. Cross-core payloads travel through bounded Pico queues and
published flags/epochs, not shared mutable C++ session objects.

Backends are compile-time parameters (static polymorphism), so ARM Release can
inline register operations. Change hardware code only in `pico_backends.hpp` or
board integration; keep wire/state tests unchanged. Measure code size and inspect
ARM objects before claiming a performance improvement. Do not change accepted
HID ordering, queue capacities or baud as an incidental optimization.

## Tests (WSL only)

From repository root: `./work/test-firmware-host.ps1`. Equivalently inside WSL:

```sh
cmake -S common/tests -B work/firmware-host-tests -DCMAKE_BUILD_TYPE=Debug
cmake --build work/firmware-host-tests --parallel
ctest --test-dir work/firmware-host-tests --output-on-failure
```

`./work/test-firmware-host.ps1 -Sanitize` runs the same WSL suite with ASan and
UBSan in a separate build directory. Keep asserts enabled for tests; production
Release still uses the normal SDK compiler optimization flags.

The mock interfaces remain portable, but firmware tests and optimization are
executed only in WSL by project policy. Tests have no USB/UART/HID device access.
Use `work/test-firmware.ps1` for the older strict C codec suite. Build actual
firmware separately with each board's `build-wsl.ps1`; its default and cache
override are **921600 bps**. Temporary baud leases restore this baseline.
The USB NCM advertised link speed intentionally remains 115200 bit/s to encourage
low OS routing preference; it is not a UART baud measurement.

MSC sector reads never wait for UART inside a TinyUSB callback. The SDK's old
read==0 path self-enqueues indefinitely; `common/msc_driver.cmake` generates a
local cooperative variant with an explicit core-1 resume function. The SDK stays
untouched and its MIT notice is retained. HID runs between storage reads. MSC
uses96 individual512-byte LRU sectors (48KiB), separate4KiB CRC staging,
16 small command slots,2 read-event slots and one large FS transfer slot.
Slave MSC is hidden until an explicit live share, and removed on OFF/eject or
5-second heartbeat expiry. NCM and MSC use one bitmask visibility coordinator:
descriptor changes happen only detached, with a shared 300ms reconnect delay.
The host suite inspects all four real TinyUSB-generated descriptor layouts.
