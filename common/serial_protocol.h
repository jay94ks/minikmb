#ifndef KBM_SERIAL_PROTOCOL_H
#define KBM_SERIAL_PROTOCOL_H
#include <stdbool.h>
#include <stdint.h>

#define KBM_SERIAL_OP_WRITE 0x30u
#define KBM_SERIAL_OP_STATUS 0x31u
#define KBM_SERIAL_OP_RX_ACK 0x32u
#define KBM_SERIAL_EVENT_RX 0xf4u
#define KBM_SERIAL_EVENT_STATE 0xf5u
#define KBM_SERIAL_FLAG_USB 0x01u
#define KBM_SERIAL_FLAG_DTR 0x02u
#define KBM_SERIAL_FLAG_RTS 0x04u
#define KBM_SERIAL_RX_BYTES 248u

static inline bool kbm_serial_tx_ready(uint8_t flags) {
    return (flags & 7u) == 7u;
}
static inline bool kbm_serial_rx_ready(uint8_t flags) {
    return (flags & 3u) == 3u;
}
#endif
