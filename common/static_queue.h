#ifndef KBM_STATIC_QUEUE_H
#define KBM_STATIC_QUEUE_H
#include "pico/util/queue.h"
#include <string.h>
// Pico queue operations/locking are retained, but storage is owned by the
// firmware. SDK queue_init() allocates capacity+1 entries with calloc.
// Never call queue_free() on these statically backed queues.
static inline void kbm_queue_init_storage(queue_t *q,void *storage,uint16_t size,uint16_t capacity) {
    memset(q,0,sizeof(*q));
    lock_init(&q->core,next_striped_spin_lock_num());
    q->data=(uint8_t *)storage;q->element_size=size;q->element_count=capacity;
}
#ifdef __cplusplus
#define KBM_QUEUE_ASSERT static_assert
#else
#define KBM_QUEUE_ASSERT _Static_assert
#endif
#define KBM_STATIC_QUEUE_INIT(q,type,capacity) do { \
    KBM_QUEUE_ASSERT(sizeof(type)<=UINT16_MAX,"queue element too large"); \
    KBM_QUEUE_ASSERT((capacity)>0 && (capacity)<UINT16_MAX,"invalid queue capacity"); \
    static type kbm_queue_storage[(capacity)+1]; \
    kbm_queue_init_storage((q),kbm_queue_storage,sizeof(type),(capacity)); \
} while(0)
#endif
