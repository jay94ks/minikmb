#ifndef KBM_REBOOT_GUARD_H
#define KBM_REBOOT_GUARD_H
#include <stdbool.h>
#include <stdint.h>
typedef struct { bool draining_complete; uint32_t reset_at; } kbm_reboot_guard_t;
// Call only after accepting an explicit RP2040 reboot request. No reset can
// happen before both the software buffer and the hardware UART have drained.
static inline bool kbm_reboot_ready(kbm_reboot_guard_t *guard, uint32_t now,
                                    bool ack_pending, bool tx_drained, bool uart_idle) {
    if (!guard->draining_complete && !ack_pending && tx_drained && uart_idle) {
        guard->draining_complete = true;
        guard->reset_at = now + 100u; // Let the USB owner detach cleanly.
    }
    return guard->draining_complete && (int32_t)(now - guard->reset_at) >= 0;
}
#endif
