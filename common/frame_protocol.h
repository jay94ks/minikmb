#ifndef KBM_FRAME_PROTOCOL_H
#define KBM_FRAME_PROTOCOL_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#define KBM_STX 0x02u
#define KBM_ETX 0x03u
#define KBM_ESC 0x1bu
#define KBM_MAX_PAYLOAD 255u
#define KBM_MAX_LARGE_PAYLOAD 4118u
#define KBM_MAX_FRAME_SIZE (KBM_MAX_PAYLOAD * 2u + 6u)
#define KBM_MAX_LARGE_FRAME_SIZE (KBM_MAX_LARGE_PAYLOAD * 2u + 6u)
// Large FS buffers are explicit; ordinary queues retain compact elements.
typedef struct { uint8_t op; uint16_t len; uint8_t payload[KBM_MAX_PAYLOAD]; } kbm_packet_t;
typedef enum { KBM_DECODE_MORE=0, KBM_DECODE_FRAME, KBM_DECODE_BAD_ESCAPE,
    KBM_DECODE_BAD_ETX, KBM_DECODE_BAD_CHECKSUM, KBM_DECODE_BAD_LENGTH,
    KBM_DECODE_LARGE_FRAME } kbm_decode_result_t;
typedef enum { KBM_DEC_WAIT_STX=0, KBM_DEC_OP, KBM_DEC_LEN, KBM_DEC_LEN_HIGH,
    KBM_DEC_PAYLOAD, KBM_DEC_PAYLOAD_ESC, KBM_DEC_ETX, KBM_DEC_CHECKSUM } kbm_decode_state_t;
typedef struct {
    kbm_decode_state_t state; kbm_packet_t packet;
    uint16_t payload_pos; uint8_t checksum;
    uint8_t *large; uint16_t capacity;
} kbm_decoder_t;
typedef void (*kbm_write_byte_fn)(uint8_t byte, void *user);
#ifdef __cplusplus
extern "C" {
#endif
void kbm_decoder_init(kbm_decoder_t *decoder);
void kbm_decoder_init_large(kbm_decoder_t *decoder, uint8_t *storage, uint16_t capacity);
kbm_decode_result_t kbm_decoder_feed(kbm_decoder_t *decoder, uint8_t byte);
uint8_t kbm_packet_checksum(const kbm_packet_t *packet);
void kbm_packet_write(const kbm_packet_t *packet, kbm_write_byte_fn write_byte, void *user);
void kbm_frame_write(uint8_t op, uint16_t len, const uint8_t *payload, kbm_write_byte_fn write_byte, void *user);
#ifdef __cplusplus
}
#endif
#endif
