#ifndef KBM_SLAVE_APP_STATE_H
#define KBM_SLAVE_APP_STATE_H

#include "pico/util/queue.h"
#include "protocol.h"

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
extern queue_t g_command_queue;
typedef struct { kbm_packet_t packet; uint32_t usb_epoch; } kbm_hid_command_t;
extern queue_t g_serial_tx_queue;
extern queue_t g_uart_event_queue;
extern queue_t g_net_event_queue, g_net_rx_queue, g_net_tx_queue;
extern queue_t g_disk_commands, g_disk_events;
extern queue_t g_disk_transfers;
extern volatile uint32_t g_disk_flags;
extern volatile uint32_t g_net_enabled, g_net_ready, g_net_credit;
extern volatile uint32_t g_serial_flags;
extern volatile uint32_t g_serial_rx_acked;
extern volatile uint32_t g_reboot_requested;
extern volatile bool g_usb_mounted;
extern volatile uint32_t g_hid_activity_generation;
extern volatile uint32_t g_hid_usb_epoch;
extern volatile uint32_t g_usb_state;
extern queue_t g_hid_diagnostics;
extern volatile uint32_t g_hid_queue_rejected,g_slave_uart_errors;
#ifdef __cplusplus
}
#endif

#endif

