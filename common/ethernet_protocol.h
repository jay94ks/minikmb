#ifndef KBM_ETHERNET_PROTOCOL_H
#define KBM_ETHERNET_PROTOCOL_H
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#define KBM_NET_WRITE 0x40u
#define KBM_NET_STATUS 0x41u
#define KBM_NET_CREDIT 0x42u
#define KBM_NET_ENABLE 0x43u
#define KBM_NET_EVENT 0xf6u
#define KBM_NET_MTU 1514u
// A low-priority transfer is preemptible at every fragment boundary.
// Worst-case escaped fragment takes <14 ms at the baseline 115200 baud.
#define KBM_NET_CHUNK 64u
#define KBM_NET_HEADER 10u
typedef struct { uint16_t length; uint8_t data[KBM_NET_MTU]; } kbm_net_frame_t;
typedef struct {
    kbm_net_frame_t frame;
    uint16_t id, offset;
    uint32_t crc;
} kbm_net_assembly_t;
static inline uint16_t kbm_net_u16(const uint8_t *p) {
    return (uint16_t)p[0] | ((uint16_t)p[1] << 8);
}
static inline uint32_t kbm_net_u32(const uint8_t *p) {
    return (uint32_t)kbm_net_u16(p) | ((uint32_t)kbm_net_u16(p + 2) << 16);
}
static inline void kbm_net_put16(uint8_t *p, uint16_t n) { p[0] = (uint8_t)n; p[1] = (uint8_t)(n >> 8); }
static inline void kbm_net_put32(uint8_t *p, uint32_t n) {
    kbm_net_put16(p, (uint16_t)n); kbm_net_put16(p + 2, (uint16_t)(n >> 16));
}
static inline uint32_t kbm_net_crc(const uint8_t *p, uint16_t n) {
    uint32_t crc = 0xffffffffu;
    while (n--) {
        crc ^= *p++;
        for (unsigned b = 0; b < 8; ++b) crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1u)));
    }
    return ~crc;
}
// 0=incomplete, 1=complete, -1=invalid; never inject a partial/corrupt frame.
static inline int kbm_net_accept(kbm_net_assembly_t *a, const uint8_t *p, uint8_t n) {
    if (n <= KBM_NET_HEADER || n > KBM_NET_HEADER + KBM_NET_CHUNK) return -1;
    const uint16_t id = kbm_net_u16(p), total = kbm_net_u16(p + 2), off = kbm_net_u16(p + 4);
    const uint32_t crc = kbm_net_u32(p + 6);
    const uint16_t count = n - KBM_NET_HEADER;
    if (total < 14 || total > KBM_NET_MTU || off + count > total) return -1;
    if (off == 0) { a->id = id; a->frame.length = total; a->offset = 0; a->crc = crc; }
    if (id != a->id || total != a->frame.length || crc != a->crc || off != a->offset) return -1;
    memcpy(a->frame.data + off, p + KBM_NET_HEADER, count);
    a->offset += count;
    if (a->offset != total) return 0;
    return kbm_net_crc(a->frame.data, total) == crc ? 1 : -1;
}
#endif
