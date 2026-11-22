#ifndef KBM_DISK_PROTOCOL_H
#define KBM_DISK_PROTOCOL_H
#include "ethernet_protocol.h"
#define KBM_DISK_CONFIG 0x44u
#define KBM_DISK_DATA 0x45u
#define KBM_DISK_STATUS 0x46u
#define KBM_DISK_FAIL 0x47u
#define KBM_DISK_READ_EVENT 0xf8u
#define KBM_DISK_SECTOR 512u
#define KBM_DISK_CHUNK 4096u
#define KBM_DISK_CACHE_SECTORS 96u
#define KBM_DISK_MAX_READ_SECTORS 8u
#define KBM_DISK_HEADER 18u
#define KBM_DISK_MAX_SECTORS 67108864u
static inline bool kbm_disk_config_valid(const uint8_t *p, uint8_t n) {
    if(n != 9 || p[0]>1) return false;
    uint32_t generation=kbm_net_u32(p+1), sectors=kbm_net_u32(p+5);
    return generation && (p[0] ? sectors && sectors<=KBM_DISK_MAX_SECTORS : sectors==0);
}
typedef struct { uint16_t len; uint8_t payload[KBM_DISK_HEADER+KBM_DISK_CHUNK]; } kbm_disk_transfer_t;
typedef struct { uint16_t sequence; kbm_disk_transfer_t transfer; } kbm_disk_route_t;
static inline bool kbm_disk_data_valid(const uint8_t *p, uint16_t n) {
    if(n<=KBM_DISK_HEADER || n>KBM_DISK_HEADER+KBM_DISK_CHUNK) return false;
    return kbm_net_u16(p+12)+(unsigned)(n-KBM_DISK_HEADER)<=KBM_DISK_CHUNK;
}
#endif
