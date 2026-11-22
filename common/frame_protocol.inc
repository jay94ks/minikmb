// Shared C ABI codec compiled into each firmware/test executable, not the SDK.
#include "frame_protocol.h"
#include <string.h>
static bool is_control(uint8_t b) { return b==KBM_STX || b==KBM_ETX || b==KBM_ESC; }
static void restart_at_stx(kbm_decoder_t *d) { d->state=KBM_DEC_OP; d->payload_pos=0; d->checksum=0; }
static void reset_after_error(kbm_decoder_t *d,uint8_t b) {
    if(b==KBM_STX) restart_at_stx(d);
    else { d->state=KBM_DEC_WAIT_STX; d->payload_pos=0; d->checksum=0; }
}
void kbm_decoder_init(kbm_decoder_t *d) { memset(d,0,sizeof(*d)); d->capacity=KBM_MAX_PAYLOAD; }
void kbm_decoder_init_large(kbm_decoder_t *d,uint8_t *storage,uint16_t capacity) {
    kbm_decoder_init(d); d->large=storage;
    d->capacity=capacity<KBM_MAX_LARGE_PAYLOAD?capacity:KBM_MAX_LARGE_PAYLOAD;
}
static void payload_byte(kbm_decoder_t *d,uint8_t b) {
    (d->large?d->large:d->packet.payload)[d->payload_pos++]=b;
    d->checksum=(uint8_t)(d->checksum+b);
    d->state=d->payload_pos==d->packet.len?KBM_DEC_ETX:KBM_DEC_PAYLOAD;
}
kbm_decode_result_t kbm_decoder_feed(kbm_decoder_t *d,uint8_t b) {
    switch(d->state) {
    case KBM_DEC_WAIT_STX: if(b==KBM_STX) restart_at_stx(d); return KBM_DECODE_MORE;
    case KBM_DEC_OP: d->packet.op=b;d->checksum=b;d->state=KBM_DEC_LEN;return KBM_DECODE_MORE;
    case KBM_DEC_LEN: d->packet.len=b;d->checksum=(uint8_t)(d->checksum+b);d->state=KBM_DEC_LEN_HIGH;return KBM_DECODE_MORE;
    case KBM_DEC_LEN_HIGH:
        d->packet.len|=(uint16_t)b<<8;d->checksum=(uint8_t)(d->checksum+b);d->payload_pos=0;
        if(d->packet.len>d->capacity) { d->state=KBM_DEC_WAIT_STX;return KBM_DECODE_BAD_LENGTH; }
        d->state=d->packet.len?KBM_DEC_PAYLOAD:KBM_DEC_ETX;return KBM_DECODE_MORE;
    case KBM_DEC_PAYLOAD:
        if(b==KBM_ESC) { d->state=KBM_DEC_PAYLOAD_ESC;return KBM_DECODE_MORE; }
        if(b==KBM_STX||b==KBM_ETX) { reset_after_error(d,b);return KBM_DECODE_BAD_ESCAPE; }
        payload_byte(d,b);return KBM_DECODE_MORE;
    case KBM_DEC_PAYLOAD_ESC:
        if(!is_control(b)) { reset_after_error(d,b);return KBM_DECODE_BAD_ESCAPE; }
        payload_byte(d,b);return KBM_DECODE_MORE;
    case KBM_DEC_ETX:
        if(b!=KBM_ETX) { reset_after_error(d,b);return KBM_DECODE_BAD_ETX; }
        d->state=KBM_DEC_CHECKSUM;return KBM_DECODE_MORE;
    case KBM_DEC_CHECKSUM:
        d->state=KBM_DEC_WAIT_STX;d->payload_pos=0;
        if(b!=d->checksum) return KBM_DECODE_BAD_CHECKSUM;
        if(d->packet.len>KBM_MAX_PAYLOAD) return KBM_DECODE_LARGE_FRAME;
        if(d->large) memcpy(d->packet.payload,d->large,d->packet.len);
        return KBM_DECODE_FRAME;
    }
    d->state=KBM_DEC_WAIT_STX;return KBM_DECODE_BAD_ETX;
}
static uint8_t frame_checksum(uint8_t op,uint16_t len,const uint8_t *p) {
    uint8_t sum=(uint8_t)(op+(len&255u)+(len>>8));
    for(uint16_t i=0;i<len;i++) sum=(uint8_t)(sum+p[i]);
    return sum;
}
uint8_t kbm_packet_checksum(const kbm_packet_t *p) { return p->len<=KBM_MAX_PAYLOAD?frame_checksum(p->op,p->len,p->payload):0; }
void kbm_frame_write(uint8_t op,uint16_t len,const uint8_t *p,kbm_write_byte_fn write,void *user) {
    if(len>KBM_MAX_LARGE_PAYLOAD) return;
    write(KBM_STX,user);write(op,user);write((uint8_t)len,user);write((uint8_t)(len>>8),user);
    for(uint16_t i=0;i<len;i++) { if(is_control(p[i])) write(KBM_ESC,user);write(p[i],user); }
    write(KBM_ETX,user);write(frame_checksum(op,len,p),user);
}
void kbm_packet_write(const kbm_packet_t *p,kbm_write_byte_fn write,void *user) {
    if(p->len<=KBM_MAX_PAYLOAD) kbm_frame_write(p->op,p->len,p->payload,write,user);
}
