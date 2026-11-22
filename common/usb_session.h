#pragma once
#include <stdbool.h>
#include <stdint.h>
// Poll real stack state; callbacks alone miss RP2040 host reset/unplug paths.
// No missing-SOF timeout: without VBUS sense, suspend is not proof of unplug.
#define KBM_USB_SESSION_RESET 1u
#define KBM_USB_SESSION_MOUNT 2u
#define KBM_USB_SESSION_SUSPEND 4u
typedef struct { bool mounted,suspended; uint32_t reset_generation; } kbm_usb_session_t;
static inline uint8_t kbm_usb_session_poll(kbm_usb_session_t *state,bool mounted,bool suspended,uint32_t reset_generation) {
    uint8_t changes=0;
    if(state->reset_generation!=reset_generation) changes|=KBM_USB_SESSION_RESET;
    if(state->mounted!=mounted) changes|=KBM_USB_SESSION_MOUNT;
    if(state->suspended!=suspended) changes|=KBM_USB_SESSION_SUSPEND;
    state->mounted=mounted;state->suspended=suspended;state->reset_generation=reset_generation;
    return changes;
}
