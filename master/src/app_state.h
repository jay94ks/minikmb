#ifndef KBM_MASTER_APP_STATE_H
#define KBM_MASTER_APP_STATE_H

#include "master_commands.h"
#include "disk_protocol.h"

#include "pico/util/queue.h"
#include <stdint.h>

typedef struct {
    volatile uint32_t tx_frames;
    volatile uint32_t rx_bytes;
    volatile uint32_t rx_frames;
    volatile uint32_t decode_errors;
} kbm_uart_diagnostics_t;

#ifdef __cplusplus
extern "C" {
#endif
extern queue_t g_uart_tx_queue[2];
extern queue_t g_uart_bulk_queue[2], g_usb_bulk_queue;
extern queue_t g_uart_disk_queue[2];
extern queue_t g_usb_tx_queue;
extern volatile uint32_t g_online_mask;
extern volatile uint32_t g_serial_flags[2];
extern kbm_uart_diagnostics_t g_uart_diagnostics[2];
typedef struct { uint8_t port; kbm_net_frame_t frame; } kbm_native_frame_t;
extern queue_t g_native_from_usb[2], g_native_to_usb;
#ifdef __cplusplus
}
#endif

#endif

