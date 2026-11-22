#ifndef KBM_REBOOT_PROTOCOL_H
#define KBM_REBOOT_PROTOCOL_H
#include <stdbool.h>
#include <stdint.h>
#define KBM_REBOOT_OP 0x60u
#define KBM_REBOOT_MAGIC_SIZE 4u
static inline bool kbm_reboot_valid(const uint8_t *payload, uint8_t length) {
    return length == KBM_REBOOT_MAGIC_SIZE && payload[0] == 'R' && payload[1] == 'P' &&
        payload[2] == '2' && payload[3] == '0';
}
#endif
