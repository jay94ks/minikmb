#include "response_tracker.h"

#include <string.h>

void kbm_response_tracker_init(kbm_response_tracker_t *tracker) {
    memset(tracker, 0, sizeof(*tracker));
}

bool kbm_response_tracker_full(const kbm_response_tracker_t *tracker) {
    return tracker->count == KBM_PENDING_CAPACITY;
}

bool kbm_response_tracker_push(kbm_response_tracker_t *tracker,
                               uint32_t sent_ms,
                               uint8_t request_op,
                               bool forward_response) {
    if (kbm_response_tracker_full(tracker)) {
        return false;
    }

    const uint8_t tail =
        (uint8_t)((tracker->head + tracker->count) % KBM_PENDING_CAPACITY);
    tracker->entries[tail] = (kbm_pending_request_t){
        .sent_ms = sent_ms,
        .expected_response_op = (uint8_t)(request_op | 0x80u),
        .forward_response = forward_response,
    };
    ++tracker->count;
    return true;
}

bool kbm_response_tracker_match(kbm_response_tracker_t *tracker,
                                uint8_t response_op,
                                kbm_pending_request_t *matched) {
    if (tracker->count == 0) {
        return false;
    }

    uint8_t skipped = 0;
    while (skipped < tracker->count) {
        const uint8_t index =
            (uint8_t)((tracker->head + skipped) % KBM_PENDING_CAPACITY);
        const kbm_pending_request_t *pending = &tracker->entries[index];
        if (response_op == pending->expected_response_op) {
            *matched = *pending;
            tracker->head =
                (uint8_t)((index + 1u) % KBM_PENDING_CAPACITY);
            tracker->count = (uint8_t)(tracker->count - skipped - 1u);
            return true;
        }
        // Only internal keepalives can be skipped. A later ordered ACK proves
        // that these PING replies were lost; host requests must never be skipped.
        if (pending->forward_response) {
            return false;
        }
        ++skipped;
    }
    return false;
}

const kbm_pending_request_t *kbm_response_tracker_oldest(
    const kbm_response_tracker_t *tracker) {
    return tracker->count == 0 ? NULL : &tracker->entries[tracker->head];
}

void kbm_response_tracker_clear(kbm_response_tracker_t *tracker) {
    tracker->head = 0;
    tracker->count = 0;
}
