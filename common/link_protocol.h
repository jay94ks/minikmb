#ifndef KBM_LINK_PROTOCOL_H
#define KBM_LINK_PROTOCOL_H
#include "ethernet_protocol.h"
#define KBM_LINK_BAUD 0x50u
#define KBM_LINK_ECHO 0x51u
static inline bool kbm_link_baud_valid(uint32_t baud) {
    return baud == 115200 || baud == 230400 || baud == 460800 || baud == 921600 || baud == 1000000;
}
static inline bool kbm_link_lease_valid(const uint8_t *p) {
    const uint16_t lease = kbm_net_u16(p + 4);
    return kbm_link_baud_valid(kbm_net_u32(p)) && lease >= 1000 && lease <= 10000;
}
#endif
