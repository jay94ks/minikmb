#include "static_queue.h"
#include "app_state.h"
#include "hid_executor.h"
#include "protocol.h"
#include "uart_transport.h"
#include "ethernet_protocol.h"
#include "network_bridge.h"
#include "disk_protocol.h"

#include "bsp/board_api.h"
#include "pico/multicore.h"

queue_t g_command_queue;
queue_t g_serial_tx_queue;
queue_t g_uart_event_queue;
queue_t g_net_event_queue, g_net_rx_queue, g_net_tx_queue;
queue_t g_disk_commands, g_disk_events;
queue_t g_disk_transfers;
volatile uint32_t g_disk_flags;
volatile uint32_t g_net_enabled, g_net_ready, g_net_credit;
volatile uint32_t g_serial_flags;
volatile uint32_t g_serial_rx_acked = 0x10000u;
volatile uint32_t g_reboot_requested;
volatile bool g_usb_mounted = false;
volatile uint32_t g_hid_activity_generation;
volatile uint32_t g_hid_usb_epoch;
volatile uint32_t g_usb_state;
queue_t g_hid_diagnostics;
volatile uint32_t g_hid_queue_rejected,g_slave_uart_errors;

int main(void) {
    board_init();
    KBM_STATIC_QUEUE_INIT(&g_command_queue,kbm_hid_command_t,32);
    KBM_STATIC_QUEUE_INIT(&g_hid_diagnostics,kbm_packet_t,1);
    KBM_STATIC_QUEUE_INIT(&g_serial_tx_queue,kbm_packet_t,64);
    KBM_STATIC_QUEUE_INIT(&g_uart_event_queue,kbm_packet_t,64);
    KBM_STATIC_QUEUE_INIT(&g_net_event_queue,kbm_packet_t,2);
    KBM_STATIC_QUEUE_INIT(&g_net_rx_queue,kbm_net_frame_t,8);
    KBM_STATIC_QUEUE_INIT(&g_net_tx_queue,kbm_net_frame_t,8);
    KBM_STATIC_QUEUE_INIT(&g_disk_commands,kbm_packet_t,16);
    KBM_STATIC_QUEUE_INIT(&g_disk_events,kbm_packet_t,2);
    KBM_STATIC_QUEUE_INIT(&g_disk_transfers,kbm_disk_transfer_t,1);
    network_bridge_init();

    multicore_launch_core1(hid_executor_core1);

    // Core 0 remains the UART owner for the lifetime of the firmware.
    uart_transport_run();
    return 0;
}

