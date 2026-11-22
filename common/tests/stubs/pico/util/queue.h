#pragma once
// Host-only lock/ABI mock. Ring operations below are compiled from the real SDK queue.c.
#include <stdbool.h>
#include <stdint.h>
#include <assert.h>
typedef unsigned uint;
#define PICO_QUEUE_MAX_LEVEL 0
typedef struct { unsigned *spin_lock; } lock_core_t;
typedef struct { lock_core_t core;uint8_t *data;uint16_t wptr,rptr,element_size,element_count; } queue_t;
static inline unsigned next_striped_spin_lock_num(void) { return 0; }
static inline void lock_init(lock_core_t *lock,unsigned number) { static unsigned spin;(void)number;lock->spin_lock=&spin; }
static inline uint32_t spin_lock_blocking(unsigned *spin) { (void)spin;return 0; }
static inline void spin_unlock(unsigned *spin,uint32_t save) { (void)spin;(void)save; }
static inline void lock_internal_spin_unlock_with_wait(lock_core_t *lock,uint32_t save) { spin_unlock(lock->spin_lock,save);assert(!"Unexpected blocking queue wait"); }
static inline void lock_internal_spin_unlock_with_notify(lock_core_t *lock,uint32_t save) { spin_unlock(lock->spin_lock,save); }
static inline uint queue_get_level_unsafe(queue_t *q) { return q->wptr>=q->rptr?q->wptr-q->rptr:q->wptr+q->element_count+1-q->rptr; }
static inline uint queue_get_level(queue_t *q) { return queue_get_level_unsafe(q); }
bool queue_try_add(queue_t *,const void *);
bool queue_try_remove(queue_t *,void *);
bool queue_try_peek(queue_t *,void *);
void queue_add_blocking(queue_t *,const void *);
void queue_remove_blocking(queue_t *,void *);
void queue_peek_blocking(queue_t *,void *);
