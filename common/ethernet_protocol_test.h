// Included by each firmware's host suite; no Pico SDK dependency.
#include "ethernet_protocol.h"
#include "link_protocol.h"
static void test_ethernet_and_link_protocol(void) {
    assert(kbm_net_crc((const uint8_t *)"123456789", 9) == 0xcbf43926u);
    kbm_net_assembly_t a = {0};
    uint8_t frame[KBM_NET_MTU], payload[KBM_NET_HEADER + KBM_NET_CHUNK];
    for (unsigned i = 0; i < sizeof(frame); ++i) frame[i] = (uint8_t)(i * 73);
    const uint32_t crc = kbm_net_crc(frame, sizeof(frame));
    for (unsigned pass = 0; pass < 2; ++pass) {
        memset(&a, 0, sizeof(a));
        for (uint16_t offset = 0; offset < sizeof(frame);) {
            const uint16_t remaining = sizeof(frame) - offset;
            const uint8_t n = remaining > KBM_NET_CHUNK ? KBM_NET_CHUNK : remaining;
            kbm_net_put16(payload, 65535); kbm_net_put16(payload + 2, sizeof(frame));
            kbm_net_put16(payload + 4, offset); kbm_net_put32(payload + 6, crc);
            memcpy(payload + KBM_NET_HEADER, frame + offset, n);
            if (pass && offset == 0) payload[KBM_NET_HEADER] ^= 1;
            const int result = kbm_net_accept(&a, payload, KBM_NET_HEADER + n);
            offset += n;
            assert(result == (offset == sizeof(frame) ? (pass ? -1 : 1) : 0));
        }
        if (!pass) assert(memcmp(a.frame.data, frame, sizeof(frame)) == 0);
    }
    assert(kbm_net_accept(&a, payload, KBM_NET_HEADER) == -1);
    kbm_net_put16(payload + 2, KBM_NET_MTU + 1);
    assert(kbm_net_accept(&a, payload, KBM_NET_HEADER + 1) == -1);
    assert(kbm_link_baud_valid(115200)); assert(kbm_link_baud_valid(1000000));
    assert(!kbm_link_baud_valid(2000000));
    kbm_net_put32(payload, 921600); kbm_net_put16(payload + 4, 10000);
    assert(kbm_link_lease_valid(payload));
    kbm_net_put16(payload + 4, 10001); assert(!kbm_link_lease_valid(payload));
}
