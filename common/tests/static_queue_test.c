#include "static_queue.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
void *__wrap_calloc(size_t n,size_t size) { (void)n;(void)size;assert(!"Queue must never allocate");return NULL; }
void __wrap_free(void *p) { (void)p;assert(!"Static queue must never free storage"); }
int main(void) {
    queue_t q;
    KBM_STATIC_QUEUE_INIT(&q,uint32_t,3);
    uint32_t in=0,out=0;
    assert(q.element_count==3&&q.element_size==4&&queue_get_level(&q)==0);
    for(unsigned round=0;round<100;round++) {
        for(unsigned n=0;n<3;n++) {in=round*3+n;assert(queue_try_add(&q,&in));}
        assert(!queue_try_add(&q,&in));assert(queue_get_level(&q)==3);
        for(unsigned n=0;n<3;n++) {assert(queue_try_remove(&q,&out));assert(out==round*3+n);}
        assert(!queue_try_remove(&q,&out));
    }
    puts("PASS static queue storage with real SDK ring operations: sentinel slot, bounds, FIFO,100 wraparounds, no calloc/free");
}
