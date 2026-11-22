#ifndef KBM_MASTER_RESPONSE_TRACKER_H
#define KBM_MASTER_RESPONSE_TRACKER_H

#include <stdbool.h>
#include <stdint.h>

#define KBM_PENDING_CAPACITY 32u

typedef struct {
    uint32_t sent_ms;
    uint8_t expected_response_op;
    bool forward_response;
} kbm_pending_request_t;

typedef struct {
    kbm_pending_request_t entries[KBM_PENDING_CAPACITY];
    uint8_t head;
    uint8_t count;
} kbm_response_tracker_t;

#ifdef __cplusplus
extern "C" {
#endif
void kbm_response_tracker_init(kbm_response_tracker_t *tracker);
bool kbm_response_tracker_full(const kbm_response_tracker_t *tracker);
bool kbm_response_tracker_push(kbm_response_tracker_t *tracker,
                               uint32_t sent_ms,
                               uint8_t request_op,
                               bool forward_response);
bool kbm_response_tracker_match(kbm_response_tracker_t *tracker,
                                uint8_t response_op,
                                kbm_pending_request_t *matched);
const kbm_pending_request_t *kbm_response_tracker_oldest(
    const kbm_response_tracker_t *tracker);
void kbm_response_tracker_clear(kbm_response_tracker_t *tracker);
#ifdef __cplusplus
}
#endif

#endif
