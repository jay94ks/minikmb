#include "protocol.h"
#include "command.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    uint8_t bytes[1024];
    size_t length;
} byte_buffer_t;

static void append_byte(uint8_t byte, void *user) {
    byte_buffer_t *buffer = user;
    assert(buffer->length < sizeof(buffer->bytes));
    buffer->bytes[buffer->length++] = byte;
}

static kbm_decode_result_t feed_all(kbm_decoder_t *decoder,
                                    const byte_buffer_t *buffer) {
    kbm_decode_result_t result = KBM_DECODE_MORE;
    for (size_t i = 0; i < buffer->length; ++i) {
        result = kbm_decoder_feed(decoder, buffer->bytes[i]);
        if (i + 1 < buffer->length) {
            assert(result == KBM_DECODE_MORE);
        }
    }
    return result;
}

static void test_progressive_escaped_payload(void) {
    const kbm_packet_t source = {
        .op = 0x20,
        .len = 6,
        .payload = {0x41, KBM_STX, KBM_ETX, KBM_ESC, 0x00, 0xff},
    };
    byte_buffer_t wire = {0};
    kbm_packet_write(&source, append_byte, &wire);

    assert(wire.length == 15);
    assert(wire.bytes[0] == KBM_STX);
    assert(wire.bytes[2] == 6 && wire.bytes[3] == 0);
    assert(wire.bytes[4] == 0x41);
    assert(wire.bytes[5] == KBM_ESC && wire.bytes[6] == KBM_STX);
    assert(wire.bytes[7] == KBM_ESC && wire.bytes[8] == KBM_ETX);
    assert(wire.bytes[9] == KBM_ESC && wire.bytes[10] == KBM_ESC);

    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    assert(feed_all(&decoder, &wire) == KBM_DECODE_FRAME);
    assert(decoder.packet.op == source.op);
    assert(decoder.packet.len == source.len);
    assert(memcmp(decoder.packet.payload, source.payload, source.len) == 0);
}

static void test_empty_and_concatenated_frames(void) {
    const kbm_packet_t first = {.op = 0x00, .len = 0};
    const kbm_packet_t second = {
        .op = 0x10,
        .len = 2,
        .payload = {0x55, 0xaa},
    };
    byte_buffer_t wire = {0};
    kbm_packet_write(&first, append_byte, &wire);
    const size_t boundary = wire.length;
    kbm_packet_write(&second, append_byte, &wire);

    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    unsigned frames = 0;
    for (size_t i = 0; i < wire.length; ++i) {
        const kbm_decode_result_t result =
            kbm_decoder_feed(&decoder, wire.bytes[i]);
        if (result == KBM_DECODE_FRAME) {
            ++frames;
            if (i + 1 == boundary) {
                assert(decoder.packet.op == first.op);
            } else {
                assert(decoder.packet.op == second.op);
            }
        } else {
            assert(result == KBM_DECODE_MORE);
        }
    }
    assert(frames == 2);
}

static void test_checksum_error_and_recovery(void) {
    const kbm_packet_t source = {
        .op = 0x20,
        .len = 1,
        .payload = {0x44},
    };
    byte_buffer_t wire = {0};
    kbm_packet_write(&source, append_byte, &wire);
    wire.bytes[wire.length - 1] ^= 0x80;

    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    assert(feed_all(&decoder, &wire) == KBM_DECODE_BAD_CHECKSUM);

    wire.length = 0;
    kbm_packet_write(&source, append_byte, &wire);
    assert(feed_all(&decoder, &wire) == KBM_DECODE_FRAME);
}

static void test_bad_escape_resynchronizes_at_stx(void) {
    const kbm_packet_t valid = {
        .op = 0x20,
        .len = 1,
        .payload = {0x42},
    };
    byte_buffer_t wire = {
        .bytes = {KBM_STX, 0x10, 0x02, 0, 0x41, KBM_STX},
        .length = 6,
    };
    byte_buffer_t encoded = {0};
    kbm_packet_write(&valid, append_byte, &encoded);

    // The final STX above becomes the start of the valid frame, so append the
    // valid frame without its leading STX.
    memcpy(&wire.bytes[wire.length], &encoded.bytes[1], encoded.length - 1);
    wire.length += encoded.length - 1;

    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    bool saw_error = false;
    bool saw_frame = false;
    for (size_t i = 0; i < wire.length; ++i) {
        const kbm_decode_result_t result =
            kbm_decoder_feed(&decoder, wire.bytes[i]);
        saw_error |= result == KBM_DECODE_BAD_ESCAPE;
        saw_frame |= result == KBM_DECODE_FRAME;
    }
    assert(saw_error);
    assert(saw_frame);
    assert(decoder.packet.op == valid.op);
    assert(decoder.packet.payload[0] == valid.payload[0]);
}

static void test_command_validation(void) {
    kbm_packet_t hid_status={.op=KBM_HID_DIAGNOSTICS_STATUS,.len=0};
    assert(kbm_command_validate(&hid_status)==KBM_STATUS_OK);
    hid_status.len=1;assert(kbm_command_validate(&hid_status)==KBM_STATUS_BAD_LENGTH);
    kbm_packet_t boot_ack={.op=KBM_BOOT_ACK,.len=KBM_BOOT_ACK_BYTES,.payload={KBM_BOOT_SCHEMA,1}};
    assert(kbm_command_validate(&boot_ack)==KBM_STATUS_OK);
    boot_ack.len=8;assert(kbm_command_validate(&boot_ack)==KBM_STATUS_BAD_VALUE);
    boot_ack.len=KBM_BOOT_ACK_BYTES;boot_ack.payload[0]=2;
    assert(kbm_command_validate(&boot_ack)==KBM_STATUS_BAD_VALUE);
    for(unsigned op=KBM_SYSTEM_DOWN;op<=KBM_SYSTEM_UP;op++) {
        kbm_packet_t system={.op=(uint8_t)op,.len=1,.payload={KBM_SYSTEM_POWER_DOWN}};
        assert(kbm_command_validate(&system)==KBM_STATUS_OK);
        system.len=0;assert(kbm_command_validate(&system)==KBM_STATUS_BAD_LENGTH);
        system.len=2;assert(kbm_command_validate(&system)==KBM_STATUS_BAD_LENGTH);
        system.len=1;system.payload[0]=0x66;assert(kbm_command_validate(&system)==KBM_STATUS_BAD_VALUE);
        unsigned count=0;
        for(unsigned usage=0;usage<256;usage++) {
            system.payload[0]=(uint8_t)usage;
            const bool valid=kbm_hid_system_index((uint8_t)usage)>=0;
            assert(kbm_command_validate(&system)==(valid?KBM_STATUS_OK:KBM_STATUS_BAD_VALUE));
            count+=valid;
        }
        assert(count==40);
    }
    kbm_packet_t packet = {
        .op = KBM_OP_KEY_DOWN,
        .len = 1,
        .payload = {0x04},
    };
    assert(kbm_command_validate(&packet) == KBM_STATUS_OK);
    packet.payload[0] = 0;
    assert(kbm_command_validate(&packet) == KBM_STATUS_BAD_VALUE);

    packet.op = KBM_OP_MOUSE_MOVE;
    packet.len = 6;
    packet.payload[0] = 0xff;
    packet.payload[1] = 0x7f;
    packet.payload[2] = 0xff;
    packet.payload[3] = 0x7f;
    packet.payload[4] = 0x7f;
    packet.payload[5] = 0x81;
    assert(kbm_command_validate(&packet) == KBM_STATUS_OK);
    packet.payload[1] = 0x80;
    assert(kbm_command_validate(&packet) == KBM_STATUS_BAD_VALUE);
    packet.payload[1] = 0x7f;
    packet.payload[4] = 0x80;
    assert(kbm_command_validate(&packet) == KBM_STATUS_BAD_VALUE);

    packet.op = KBM_OP_MOUSE_DOWN;
    packet.len = 1;
    packet.payload[0] = 1;
    assert(kbm_command_validate(&packet) == KBM_STATUS_OK);
    packet.payload[0] = 0x20;
    assert(kbm_command_validate(&packet) == KBM_STATUS_BAD_VALUE);
}

static void test_serial_commands_and_readiness(void) {
    kbm_packet_t packet = {.op = KBM_SERIAL_OP_WRITE, .len = 1, .payload = {KBM_ESC}};
    assert(kbm_command_validate(&packet) == KBM_STATUS_OK);
    packet.len = 0;
    assert(kbm_command_validate(&packet) == KBM_STATUS_BAD_LENGTH);
    packet.len = 251;
    assert(kbm_command_validate(&packet) == KBM_STATUS_OK);
    packet.len = 252;
    assert(kbm_command_validate(&packet) == KBM_STATUS_BAD_LENGTH);
    packet.op = KBM_SERIAL_OP_STATUS;
    packet.len = 0;
    assert(kbm_command_validate(&packet) == KBM_STATUS_OK);
    packet.len = 1;
    assert(kbm_command_validate(&packet) == KBM_STATUS_BAD_LENGTH);
    packet.op = KBM_SERIAL_OP_RX_ACK;
    packet.len = 2;
    assert(kbm_command_validate(&packet) == KBM_STATUS_OK);
    packet.len = 1;
    assert(kbm_command_validate(&packet) == KBM_STATUS_BAD_LENGTH);
    for (uint8_t flags = 0; flags < 8; ++flags) {
        assert(kbm_serial_tx_ready(flags) == (flags == 7));
        assert(kbm_serial_rx_ready(flags) == ((flags & 3) == 3));
    }
    packet = (kbm_packet_t){.op = KBM_SERIAL_EVENT_RX, .len = 5,
                            .payload = {KBM_STX, KBM_ETX, 0, KBM_ESC, 255}};
    byte_buffer_t wire = {0};
    kbm_packet_write(&packet, append_byte, &wire);
    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    assert(feed_all(&decoder, &wire) == KBM_DECODE_FRAME);
    assert(memcmp(decoder.packet.payload, packet.payload, packet.len) == 0);
}

#include "ethernet_protocol_test.h"
#include "frame_protocol_test.h"
static void test_network_command_validation(void) {
    kbm_packet_t wake = {.op=KBM_WAKE_OP,.len=0};
    assert(kbm_command_validate(&wake)==KBM_STATUS_OK);
    wake.len=1;assert(kbm_command_validate(&wake)==KBM_STATUS_BAD_LENGTH);
    wake=(kbm_packet_t){.op=KBM_WAKE_OP|0x80u,.len=2,.payload={0,KBM_WAKE_IGNORED_DISCONNECTED}};
    byte_buffer_t wake_wire={0};kbm_packet_write(&wake,append_byte,&wake_wire);
    kbm_decoder_t wake_decoder;kbm_decoder_init(&wake_decoder);
    assert(feed_all(&wake_decoder,&wake_wire)==KBM_DECODE_FRAME);
    assert(wake_decoder.packet.payload[0]==0&&wake_decoder.packet.payload[1]==3);
    kbm_packet_t p = {.op = KBM_NET_STATUS, .len = 0};
    assert(kbm_command_validate(&p) == KBM_STATUS_OK);
    p.op = KBM_NET_WRITE; p.len = KBM_NET_HEADER + KBM_NET_CHUNK;
    assert(kbm_command_validate(&p) == KBM_STATUS_OK);
    p.len++; assert(kbm_command_validate(&p) == KBM_STATUS_BAD_LENGTH);
    p.op = KBM_NET_ENABLE; p.len = 1; p.payload[0] = 2;
    assert(kbm_command_validate(&p) == KBM_STATUS_BAD_VALUE);
    p.payload[0] = 1; assert(kbm_command_validate(&p) == KBM_STATUS_OK);
    p.op = KBM_NET_CREDIT; p.len = 4;
    assert(kbm_command_validate(&p) == KBM_STATUS_OK);
    p.op = KBM_LINK_ECHO; p.len = 248;
    assert(kbm_command_validate(&p) == KBM_STATUS_OK);
    p.len = 249; assert(kbm_command_validate(&p) == KBM_STATUS_BAD_LENGTH);
    p = (kbm_packet_t){.op = KBM_REBOOT_OP, .len = 4, .payload = {'R','P','2','0'}};
    assert(kbm_command_validate(&p) == KBM_STATUS_OK);
    p.len = 3; assert(kbm_command_validate(&p) == KBM_STATUS_BAD_LENGTH);
    p.len = 4; p.payload[3] = '1'; assert(kbm_command_validate(&p) == KBM_STATUS_BAD_VALUE);
    p=(kbm_packet_t){.op=KBM_DISK_CONFIG,.len=9,.payload={1,1,0,0,0,0,4,0,0}};
    assert(kbm_command_validate(&p)==KBM_STATUS_OK);
    p.payload[0]=2; assert(kbm_command_validate(&p)==KBM_STATUS_BAD_VALUE);
    p.op=KBM_DISK_DATA; p.len=82; memset(p.payload,0,sizeof(p.payload));
    assert(kbm_command_validate(&p)==KBM_STATUS_OK);
    kbm_net_put16(p.payload+12,4090); assert(kbm_command_validate(&p)==KBM_STATUS_BAD_VALUE);
    p.op=KBM_DISK_FAIL; p.len=12; assert(kbm_command_validate(&p)==KBM_STATUS_OK);
    p.op=KBM_DISK_STATUS; p.len=0; assert(kbm_command_validate(&p)==KBM_STATUS_OK);
}
int main(void) {
    test_frame16();
    test_ethernet_and_link_protocol();
    test_network_command_validation();
    test_progressive_escaped_payload();
    test_empty_and_concatenated_frames();
    test_checksum_error_and_recovery();
    test_bad_escape_resynchronizes_at_stx();
    test_command_validation();
    test_serial_commands_and_readiness();
    puts("protocol tests passed");
    return 0;
}

