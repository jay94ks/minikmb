#pragma once
#include <stdbool.h>
#include <stdint.h>
#define KBM_HID_DIAGNOSTICS_EVENT 0xfau
#define KBM_HID_DIAGNOSTICS_STATUS 0x63u
#define KBM_HID_DIAGNOSTICS_BYTES 52u
#define KBM_HID_DIAGNOSTICS_SCHEMA 1u
#define KBM_DIAG_MOUNTED 1u
#define KBM_DIAG_READY 2u
#define KBM_DIAG_SUSPENDED 4u
#define KBM_DIAG_HID_READY 8u
#define KBM_DIAG_STABLE 16u
#define KBM_DIAG_WAITING 32u
#define KBM_DIAG_SUBMITTED 64u
static inline bool kbm_hid_diagnostics_valid(const uint8_t *p,uint16_t n) {
    return n==KBM_HID_DIAGNOSTICS_BYTES && p[0]==KBM_HID_DIAGNOSTICS_SCHEMA &&
        !(p[1]&0x80u) && p[2]<=32 && p[3]<=3 && p[5]==0 && p[6]==0 && p[7]==0;
}
