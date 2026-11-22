#pragma once
#define KBM_WAKE_OP 0x61u
// ACK E1: status(0=valid command), result. Ignored outcomes are NOT errors.
typedef enum {
    KBM_WAKE_SENT = 0,
    KBM_WAKE_IGNORED_ACTIVE = 1,
    KBM_WAKE_IGNORED_HOST_DISABLED = 2,
    KBM_WAKE_IGNORED_DISCONNECTED = 3,
    KBM_WAKE_IGNORED_TRANSITION = 4,
    KBM_WAKE_IGNORED_UNSUPPORTED = 5,
    KBM_WAKE_TIMED_OUT = 6, // Completion unknown, NOT an unsupported-device result.
    KBM_WAKE_PENDING = 255 // Internal only; never a successful wire response.
} kbm_wake_result_t;
