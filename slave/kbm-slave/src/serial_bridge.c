#include "serial_bridge.h"
#include "app_state.h"
#include "protocol.h"
#include "serial_protocol.h"
#include "network_bridge.h"
#include "pico/stdlib.h"
#include "tusb.h"

void serial_bridge_task(void) {
    static uint8_t reported_flags = 0xff;
    static kbm_packet_t tx;
    static uint16_t tx_offset;
    static bool tx_active;
    static kbm_packet_t rx;
    static uint16_t rx_sequence;
    static bool rx_pending;
    static bool rx_sent;
    static uint32_t last_rx_send_ms;

    const bool ready = network_bridge_usb_available() && tud_mounted() && tud_ready();
    const uint8_t lines = ready ? tud_cdc_get_line_state() : 0;
    const uint8_t flags = (ready ? KBM_SERIAL_FLAG_USB : 0) |
        ((lines & 1u) ? KBM_SERIAL_FLAG_DTR : 0) |
        ((lines & 2u) ? KBM_SERIAL_FLAG_RTS : 0);
    __atomic_store_n(&g_serial_flags, flags, __ATOMIC_RELEASE);
    if (flags != reported_flags) {
        const kbm_packet_t event = {
            .op = KBM_SERIAL_EVENT_STATE, .len = 1, .payload = {flags},
        };
        if (queue_try_add(&g_uart_event_queue, &event)) reported_flags = flags;
    }

    if (rx_pending && rx_sent &&
        __atomic_load_n(&g_serial_rx_acked, __ATOMIC_ACQUIRE) == rx_sequence) {
        rx_pending = false;
    }
    if (!rx_pending && kbm_serial_rx_ready(flags) && tud_cdc_available()) {
        ++rx_sequence;
        // Mark unacknowledged before publishing, including uint16 wraparound.
        __atomic_store_n(&g_serial_rx_acked, 0x10000u, __ATOMIC_RELEASE);
        rx.op = KBM_SERIAL_EVENT_RX;
        rx.payload[0] = (uint8_t)rx_sequence;
        rx.payload[1] = (uint8_t)(rx_sequence >> 8u);
        const uint32_t count = tud_cdc_read(&rx.payload[2], KBM_SERIAL_RX_BYTES);
        rx.len = (uint8_t)(count + 2u);
        rx_pending = count != 0;
        rx_sent = false;
    }
    const uint32_t now_ms = to_ms_since_boot(get_absolute_time());
    if (rx_pending && (!rx_sent || (uint32_t)(now_ms - last_rx_send_ms) >= 500u)) {
        if (queue_try_add(&g_uart_event_queue, &rx)) {
            rx_sent = true;
            last_rx_send_ms = now_ms;
        }
    }

    // Separate from HID report completion: serial and HID must not block each other.
    if (!kbm_serial_tx_ready(flags)) return;
    if (!tx_active && queue_try_remove(&g_serial_tx_queue, &tx)) {
        tx_active = true;
        tx_offset = 0;
    }
    if (tx_active) {
        uint32_t count = tx.len - tx_offset;
        const uint32_t available = tud_cdc_write_available();
        if (count > available) count = available;
        if (count) tx_offset += (uint16_t)tud_cdc_write(&tx.payload[tx_offset], count);
        tud_cdc_write_flush();
        if (tx_offset == tx.len) tx_active = false;
    }
}
