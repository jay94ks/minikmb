#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#define KBM_BOOT_EVENT 0xf9u
#define KBM_BOOT_ACK 0x62u
#define KBM_BOOT_SCHEMA 1u
#define KBM_BOOT_BYTES 16u
#define KBM_BOOT_TOKEN_OFFSET 4u
#define KBM_BOOT_TOKEN_BYTES 8u
#define KBM_BOOT_ACK_BYTES 9u
#define KBM_BRIDGE_SLAVE_RESTARTED 0x0bu
// Report: schema,wire-version,fw:u16le,nonce8,reset-reason,USB flags,HID depth,reserved.
static inline bool kbm_boot_valid(const uint8_t *p,uint16_t n) {
    if(n!=KBM_BOOT_BYTES || p[0]!=KBM_BOOT_SCHEMA || p[1]!=3 || p[15]!=0) return false;
    uint8_t any=0;for(unsigned i=0;i<KBM_BOOT_TOKEN_BYTES;i++) any|=p[KBM_BOOT_TOKEN_OFFSET+i];
    return any!=0;
}
static inline bool kbm_boot_ack_valid(const uint8_t *p,uint16_t n) {
    return n==KBM_BOOT_ACK_BYTES && p[0]==KBM_BOOT_SCHEMA;
}
