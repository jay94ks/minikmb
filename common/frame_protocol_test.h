#ifndef KBM_FRAME_PROTOCOL_TEST_H
#define KBM_FRAME_PROTOCOL_TEST_H
#include "frame_protocol.h"
#include <assert.h>
#include <string.h>
typedef struct { uint8_t bytes[KBM_MAX_LARGE_FRAME_SIZE];size_t n; } kbm_test_wire_t;
static void kbm_test_append(uint8_t b,void *user) {
    kbm_test_wire_t *wire=user;assert(wire->n<sizeof(wire->bytes));wire->bytes[wire->n++]=b;
}
static void test_frame16(void) {
    static uint8_t payload[KBM_MAX_LARGE_PAYLOAD], guarded[KBM_MAX_LARGE_PAYLOAD+2];
    static kbm_test_wire_t wire;
    const uint16_t lengths[]={0,1,64,255,256,530,4114,4118};
    for(unsigned k=0;k<sizeof(lengths)/sizeof(lengths[0]);k++) {
        const uint16_t n=lengths[k];uint8_t sum=(uint8_t)(0x45+(n&255)+(n>>8));
        for(uint16_t i=0;i<n;i++) {payload[i]=(const uint8_t[]){2,3,27,0,255}[i%5];sum=(uint8_t)(sum+payload[i]);}
        wire.n=0;kbm_frame_write(0x45,n,payload,kbm_test_append,&wire);
        assert(wire.bytes[2]==(uint8_t)n&&wire.bytes[3]==(uint8_t)(n>>8)&&wire.bytes[wire.n-1]==sum);
        guarded[0]=0xa5;guarded[sizeof(guarded)-1]=0x5a;
        kbm_decoder_t decoder;kbm_decoder_init_large(&decoder,guarded+1,KBM_MAX_LARGE_PAYLOAD);
        for(size_t i=0;i<wire.n;i++) {
            const kbm_decode_result_t result=kbm_decoder_feed(&decoder,wire.bytes[i]);
            assert(result==(i+1==wire.n?(n>255?KBM_DECODE_LARGE_FRAME:KBM_DECODE_FRAME):KBM_DECODE_MORE));
        }
        assert(!memcmp(guarded+1,payload,n));assert(guarded[0]==0xa5&&guarded[sizeof(guarded)-1]==0x5a);
        wire.bytes[wire.n-1]^=1;kbm_decoder_init_large(&decoder,guarded+1,KBM_MAX_LARGE_PAYLOAD);
        kbm_decode_result_t result=KBM_DECODE_MORE;
        for(size_t i=0;i<wire.n;i++) result=kbm_decoder_feed(&decoder,wire.bytes[i]);
        assert(result==KBM_DECODE_BAD_CHECKSUM);
    }
    kbm_decoder_t decoder;kbm_decoder_init(&decoder);
    const uint8_t bad[]={KBM_STX,0x45,0xff,0xff};
    for(unsigned i=0;i<3;i++) assert(kbm_decoder_feed(&decoder,bad[i])==KBM_DECODE_MORE);
    assert(kbm_decoder_feed(&decoder,bad[3])==KBM_DECODE_BAD_LENGTH);
    kbm_packet_t ping={.op=0,.len=0};wire.n=0;kbm_packet_write(&ping,kbm_test_append,&wire);
    for(size_t i=0;i<wire.n;i++) assert(kbm_decoder_feed(&decoder,wire.bytes[i])==(i+1==wire.n?KBM_DECODE_FRAME:KBM_DECODE_MORE));
}
#endif
