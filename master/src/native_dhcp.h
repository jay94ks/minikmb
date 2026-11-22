#ifndef KBM_NATIVE_DHCP_H
#define KBM_NATIVE_DHCP_H
#include "ethernet_protocol.h"
// Ingress 0/1 = slave UART, 2 = master USB host. No dynamic allocation.
typedef struct { uint8_t mac[6], ingress, address; uint32_t deadline; bool used; } kbm_dhcp_lease_t;
typedef struct { uint8_t mac[6]; kbm_dhcp_lease_t leases[8]; } kbm_dhcp_t;
#ifdef __cplusplus
extern "C" {
#endif
void kbm_dhcp_init(kbm_dhcp_t *server, const uint8_t mac[6]);
void kbm_dhcp_forget(kbm_dhcp_t *server, uint8_t ingress);
// Returns true for intercepted DHCP (even malformed). reply.length == 0 means no reply.
// Only UDP -> 67 is intercepted. Address .1 and its ARP belong to the master PC.
bool kbm_dhcp_process(kbm_dhcp_t *server, uint8_t ingress, uint32_t now,
                      const kbm_net_frame_t *request, kbm_net_frame_t *reply);
#ifdef __cplusplus
}
#endif
#endif
