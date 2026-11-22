#include "static_queue.h"
#include "app_state.h"
#include "cdc_transport.h"
#include "master_commands.h"
#include "uart_bridge.h"
#include "native_network.h"

#include "bsp/board_api.h"
#include "pico/multicore.h"
#include "pico/stdlib.h"

queue_t g_uart_tx_queue[2];
queue_t g_uart_bulk_queue[2], g_usb_bulk_queue;
queue_t g_uart_disk_queue[2];
queue_t g_usb_tx_queue;
queue_t g_native_from_usb[2], g_native_to_usb;
volatile uint32_t g_online_mask = 0;
volatile uint32_t g_serial_flags[2];
kbm_uart_diagnostics_t g_uart_diagnostics[2];

int main(void) {
    board_init();
    KBM_STATIC_QUEUE_INIT(&g_uart_tx_queue[0],kbm_bridge_request_t,64);
    KBM_STATIC_QUEUE_INIT(&g_uart_tx_queue[1],kbm_bridge_request_t,64);
    KBM_STATIC_QUEUE_INIT(&g_uart_bulk_queue[0],kbm_bridge_request_t,16);
    KBM_STATIC_QUEUE_INIT(&g_uart_bulk_queue[1],kbm_bridge_request_t,16);
    KBM_STATIC_QUEUE_INIT(&g_uart_disk_queue[0],kbm_disk_route_t,1);
    KBM_STATIC_QUEUE_INIT(&g_uart_disk_queue[1],kbm_disk_route_t,1);
    KBM_STATIC_QUEUE_INIT(&g_usb_bulk_queue,kbm_packet_t,16);
    KBM_STATIC_QUEUE_INIT(&g_usb_tx_queue,kbm_packet_t,64);
    KBM_STATIC_QUEUE_INIT(&g_native_from_usb[0],kbm_net_frame_t,8);
    KBM_STATIC_QUEUE_INIT(&g_native_from_usb[1],kbm_net_frame_t,8);
    KBM_STATIC_QUEUE_INIT(&g_native_to_usb,kbm_native_frame_t,8);
    native_network_init();
    multicore_launch_core1(uart_bridge_core1);
    cdc_transport_run();
    return 0;
}

