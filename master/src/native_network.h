#ifndef KBM_NATIVE_NETWORK_H
#define KBM_NATIVE_NETWORK_H
#include "protocol.h"
#include "ethernet_protocol.h"
#include <stdbool.h>
// All TinyUSB/descriptors/DHCP ownership is core 0. UART adaptation is core 1.
#ifdef __cplusplus
extern "C" {
#endif
void native_network_init(void);
void native_network_task(void);
void native_network_request(bool enabled, bool dhcp);
void native_network_ack_sent(void);
bool native_network_owned(void);
bool native_network_changing(void);
void native_network_status(uint8_t status, kbm_packet_t *response);
extern volatile uint32_t g_native_mode, g_native_dhcp, g_native_epoch, g_native_transition;
extern volatile uint32_t g_native_link_mask, g_native_reset_mask;
void kbm_usb_set_ncm_visible(bool visible);
bool kbm_usb_ncm_visible(void);
#ifdef __cplusplus
}
#endif
#endif
