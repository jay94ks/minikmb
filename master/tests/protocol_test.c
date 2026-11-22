#include "master_commands.h"
#include "protocol.h"
#include "response_tracker.h"
#include "wake_protocol.h"
#include "hid_controls.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    uint8_t data[1024];
    size_t length;
} buffer_t;

static void append(uint8_t byte, void *user) {
    buffer_t *buffer = user;
    assert(buffer->length < sizeof(buffer->data));
    buffer->data[buffer->length++] = byte;
}

static void test_progressive_round_trip(void) {
    const kbm_packet_t source = {
        .op = 0x20,
        .len = 6,
        .payload = {0x41, KBM_STX, KBM_ETX, KBM_ESC, 0x00, 0xff},
    };
    buffer_t wire = {0};
    kbm_packet_write(&source, append, &wire);
    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    unsigned frames = 0;
    for (size_t i = 0; i < wire.length; ++i) {
        const kbm_decode_result_t result =
            kbm_decoder_feed(&decoder, wire.data[i]);
        if (result == KBM_DECODE_FRAME) ++frames;
        else assert(result == KBM_DECODE_MORE);
    }
    assert(frames == 1);
    assert(decoder.packet.op == source.op);
    assert(decoder.packet.len == source.len);
    assert(memcmp(decoder.packet.payload, source.payload, source.len) == 0);
}

static void test_split_and_concatenated_frames(void) {
    const kbm_packet_t first = {.op = KBM_OP_STATUS, .len = 0};
    const kbm_packet_t second = {
        .op = KBM_OP_ROUTE, .len = 4, .payload = {1, 2, 3, KBM_OP_PING},
    };
    buffer_t wire = {0};
    kbm_packet_write(&first, append, &wire);
    kbm_packet_write(&second, append, &wire);
    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    unsigned frames = 0;
    for (size_t i = 0; i < wire.length; ++i) {
        if (kbm_decoder_feed(&decoder, wire.data[i]) == KBM_DECODE_FRAME) {
            ++frames;
        }
    }
    assert(frames == 2);
    assert(decoder.packet.op == KBM_OP_ROUTE);
    assert(decoder.packet.payload[0] == 1);
}

static void test_checksum_recovery(void) {
    const kbm_packet_t packet = {.op = KBM_OP_STATUS, .len = 0};
    buffer_t wire = {0};
    kbm_packet_write(&packet, append, &wire);
    wire.data[wire.length - 1] ^= 1;
    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    kbm_decode_result_t result = KBM_DECODE_MORE;
    for (size_t i = 0; i < wire.length; ++i) {
        result = kbm_decoder_feed(&decoder, wire.data[i]);
    }
    assert(result == KBM_DECODE_BAD_CHECKSUM);
    wire.length = 0;
    kbm_packet_write(&packet, append, &wire);
    for (size_t i = 0; i < wire.length; ++i) {
        result = kbm_decoder_feed(&decoder, wire.data[i]);
    }
    assert(result == KBM_DECODE_FRAME);
}

static void test_local_commands(void) {
    const kbm_packet_t internal_boot_ack={.op=KBM_OP_ROUTE,.len=4,.payload={0,1,0,KBM_BOOT_ACK}};
    kbm_bridge_request_t boot_request;
    assert(kbm_master_decode_route(&internal_boot_ack,&boot_request)==KBM_STATUS_BAD_VALUE);
    kbm_packet_t boot_cancel;
    kbm_master_wrap_response(1,0x4567,KBM_STATUS_SLAVE_RESTARTED,NULL,&boot_cancel);
    assert(boot_cancel.len==5&&boot_cancel.payload[3]==0x0b&&boot_cancel.payload[4]==0);
    for(unsigned op=KBM_SYSTEM_DOWN;op<=KBM_SYSTEM_UP;op++) {
        kbm_packet_t power={.op=KBM_OP_ROUTE,.len=5,.payload={0,7,0,(uint8_t)op,KBM_SYSTEM_POWER_DOWN}};
        kbm_bridge_request_t request;
        assert(kbm_master_decode_route(&power,&request)==KBM_STATUS_OK);
        assert(request.packet.op==op&&request.packet.len==1&&request.packet.payload[0]==0x81);
        unsigned controls=0;
        for(unsigned usage=0;usage<256;usage++) if(kbm_hid_system_index((uint8_t)usage)>=0) {
            power.payload[4]=(uint8_t)usage;
            assert(kbm_master_decode_route(&power,&request)==KBM_STATUS_OK);
            assert(request.packet.op==op&&request.packet.payload[0]==usage);
            ++controls;
        }
        assert(controls==40);
    }
    kbm_packet_t wake_route={.op=KBM_OP_ROUTE,.len=4,.payload={1,5,0,KBM_WAKE_OP}};
    kbm_bridge_request_t wake_request;
    assert(kbm_master_decode_route(&wake_route,&wake_request)==KBM_STATUS_OK);
    assert(wake_request.port==1&&wake_request.packet.op==KBM_WAKE_OP&&wake_request.packet.len==0);
    const kbm_packet_t wake_ack={.op=KBM_WAKE_OP|0x80u,.len=2,.payload={0,KBM_WAKE_IGNORED_HOST_DISABLED}};
    kbm_master_wrap_response(1,5,KBM_STATUS_OK,&wake_ack,&wake_route);
    assert(wake_route.len==7&&wake_route.payload[4]==0xe1&&wake_route.payload[5]==0&&wake_route.payload[6]==2);
    kbm_packet_t packet = {.op = KBM_OP_STATUS, .len = 0};
    assert(kbm_master_validate_local(&packet) == KBM_STATUS_OK);
    packet.len = 1;
    assert(kbm_master_validate_local(&packet) == KBM_STATUS_BAD_LENGTH);
    packet.op = 0x70;
    assert(!kbm_master_is_local(packet.op));
    assert(kbm_master_validate_local(&packet) == KBM_STATUS_UNKNOWN_OPCODE);
    assert(KBM_PROTOCOL_VERSION == 3);
    assert(KBM_EVENT_ONLINE_CHANGED == 0xf2);
    assert(KBM_ONLINE_MASK_ALL == 0x03);
    packet = (kbm_packet_t){.op = KBM_OP_NATIVE_MODE, .len = 0};
    assert(kbm_master_is_local(packet.op));
    assert(kbm_master_validate_local(&packet) == KBM_STATUS_OK);
    packet.len = 1; assert(kbm_master_validate_local(&packet) == KBM_STATUS_BAD_LENGTH);
    packet.len = 2;
    for (unsigned mode = 0; mode < 2; ++mode) for (unsigned dhcp = 0; dhcp < 2; ++dhcp) {
        packet.payload[0] = (uint8_t)mode; packet.payload[1] = (uint8_t)dhcp;
        assert(kbm_master_validate_local(&packet) == KBM_STATUS_OK);
    }
    packet.payload[1] = 2; assert(kbm_master_validate_local(&packet) == KBM_STATUS_BAD_VALUE);
    kbm_bridge_request_t request;
    packet = (kbm_packet_t){.op = KBM_OP_ROUTE, .len = 8, .payload = {1,1,0,0x60,'R','P','2','0'}};
    assert(kbm_master_decode_route(&packet, &request) == KBM_STATUS_OK);
    assert(request.port == 1 && request.packet.op == 0x60 && request.packet.len == 4);
    kbm_packet_t ack = {.op = 0xe0, .len = 2};
    kbm_master_wrap_response(1, 1, KBM_STATUS_OK, &ack, &packet);
    assert(packet.payload[4] == 0xe0 && packet.payload[5] == 0);
    packet=(kbm_packet_t){.op=KBM_OP_ROUTE,.len=4,.payload={0,1,0,0x44}};
    assert(kbm_master_decode_route(&packet,&request)==KBM_STATUS_OK);
    packet.payload[3]=0x45; assert(kbm_master_decode_route(&packet,&request)==KBM_STATUS_OK);
}

static void test_routed_requests_and_responses(void) {
    kbm_packet_t source = {
        .op = KBM_OP_ROUTE, .len = 7,
        .payload = {0, 2, 3, 0x55, KBM_STX, KBM_ETX, KBM_ESC},
    };
    // An escaped port/request-id/payload must survive byte-progressive framing.
    buffer_t wire = {0};
    kbm_packet_write(&source, append, &wire);
    kbm_decoder_t decoder;
    kbm_decoder_init(&decoder);
    for (size_t i = 0; i < wire.length; ++i) {
        const kbm_decode_result_t result = kbm_decoder_feed(&decoder, wire.data[i]);
        assert(result == (i + 1 == wire.length ? KBM_DECODE_FRAME : KBM_DECODE_MORE));
    }
    kbm_bridge_request_t requests[2];
    for (uint8_t port = 0; port < 2; ++port) {
        decoder.packet.payload[0] = port;
        assert(kbm_master_decode_route(&decoder.packet, &requests[port]) == KBM_STATUS_OK);
        assert(requests[port].port == port);
        assert(requests[port].sequence == 0x0302);
        assert(requests[port].packet.op == 0x55);
        assert(requests[port].packet.len == 3);
        assert(memcmp(requests[port].packet.payload, &source.payload[4], 3) == 0);
    }
    const kbm_packet_t slave = {.op = 0xd5, .len = 2, .payload = {1, 27}};
    for (int port = 1; port >= 0; --port) {
        kbm_packet_t response;
        kbm_master_wrap_response((uint8_t)port, requests[port].sequence,
                                 KBM_STATUS_OK, &slave, &response);
        assert(response.op == 0xf3 && response.len == 7);
        const uint8_t expected[] = {(uint8_t)port, 2, 3, 0, 0xd5, 1, 27};
        assert(memcmp(response.payload, expected, sizeof(expected)) == 0);
    }
    kbm_packet_t timeout;
    kbm_master_wrap_response(0, 0x1234, KBM_STATUS_UART_TIMEOUT, NULL, &timeout);
    assert(timeout.len == 5 && timeout.payload[3] == 9);
    assert(timeout.payload[1] == 0x34 && timeout.payload[2] == 0x12);

    source.len = 3;
    assert(kbm_master_decode_route(&source, &requests[0]) == KBM_STATUS_BAD_LENGTH);
    source.len = 4;
    source.payload[0] = 2;
    assert(kbm_master_decode_route(&source, &requests[0]) == KBM_STATUS_BAD_VALUE);
    source.payload[0] = 0;
    const uint8_t invalid_ops[] = {0x70, KBM_OP_STATUS, KBM_OP_ROUTE,
                                  0x74, 0x75, KBM_SERIAL_OP_RX_ACK, 0x80, 0xf2};
    for (size_t i = 0; i < sizeof(invalid_ops); ++i) {
        source.payload[3] = invalid_ops[i];
        assert(kbm_master_decode_route(&source, &requests[0]) == KBM_STATUS_BAD_VALUE);
    }
    source.payload[3] = KBM_OP_PING;
    source.len = 255;
    assert(kbm_master_decode_route(&source, &requests[0]) == KBM_STATUS_OK);
    assert(requests[0].packet.len == 251);
    kbm_packet_t oversized = {.op = 0x80, .len = 251};
    kbm_master_wrap_response(0, 0, KBM_STATUS_OK, &oversized, &timeout);
    assert(timeout.len == 5 && timeout.payload[3] == KBM_STATUS_BAD_LENGTH);
}

static void test_response_opcode_correlation(void) {
    kbm_response_tracker_t tracker;
    kbm_pending_request_t matched;
    kbm_response_tracker_init(&tracker);

    assert(kbm_response_tracker_push(&tracker, 10, 0x7f, true));
    assert(!kbm_response_tracker_match(&tracker, 0x80, &matched));
    assert(tracker.count == 1);
    assert(kbm_response_tracker_match(&tracker, 0xff, &matched));
    assert(matched.forward_response);
    assert(tracker.count == 0);

    assert(kbm_response_tracker_push(&tracker, 20, KBM_OP_PING, false));
    assert(kbm_response_tracker_push(&tracker, 21, 0x10, true));
    assert(kbm_response_tracker_match(&tracker, 0x80, &matched));
    assert(!matched.forward_response);
    assert(!kbm_response_tracker_match(&tracker, 0x80, &matched));
    assert(kbm_response_tracker_match(&tracker, 0x90, &matched));
    assert(matched.forward_response);

    assert(kbm_response_tracker_push(&tracker, 30, KBM_OP_PING, false));
    assert(kbm_response_tracker_oldest(&tracker)->sent_ms == 30);
    kbm_response_tracker_clear(&tracker);
    assert(kbm_response_tracker_oldest(&tracker) == NULL);

    // A lost keepalive response must not swallow the following key-down ACK.
    assert(kbm_response_tracker_push(&tracker, 40, KBM_OP_PING, false));
    assert(kbm_response_tracker_push(&tracker, 41, KBM_OP_PING, false));
    assert(kbm_response_tracker_push(&tracker, 42, 0x10, true));
    assert(!kbm_response_tracker_match(&tracker, 0xff, &matched));
    assert(tracker.count == 3);
    assert(kbm_response_tracker_match(&tracker, 0x90, &matched));
    assert(matched.forward_response && matched.sent_ms == 42);
    assert(tracker.count == 0);

    // A late PING cannot consume a host request or skip one for another ACK.
    assert(kbm_response_tracker_push(&tracker, 50, 0x10, true));
    assert(kbm_response_tracker_push(&tracker, 51, 0x11, true));
    assert(!kbm_response_tracker_match(&tracker, 0x80, &matched));
    assert(!kbm_response_tracker_match(&tracker, 0x91, &matched));
    assert(tracker.count == 2);
    assert(kbm_response_tracker_match(&tracker, 0x90, &matched));
    assert(kbm_response_tracker_match(&tracker, 0x91, &matched));

    // Exercise ring wraparound while skipping an internal PING.
    kbm_response_tracker_init(&tracker);
    for (unsigned i = 0; i < KBM_PENDING_CAPACITY - 1; ++i) {
        assert(kbm_response_tracker_push(&tracker, i, KBM_OP_PING, false));
        assert(kbm_response_tracker_match(&tracker, 0x80, &matched));
    }
    assert(kbm_response_tracker_push(&tracker, 60, KBM_OP_PING, false));
    assert(kbm_response_tracker_push(&tracker, 61, 0x10, true));
    assert(kbm_response_tracker_match(&tracker, 0x90, &matched));
    assert(tracker.count == 0);
}

#include "ethernet_protocol_test.h"
#include "frame_protocol_test.h"
int main(void) {
    test_frame16();
    test_ethernet_and_link_protocol();
    test_progressive_round_trip();
    test_split_and_concatenated_frames();
    test_checksum_recovery();
    test_local_commands();
    test_routed_requests_and_responses();
    test_response_opcode_correlation();
    puts("master protocol tests passed");
    return 0;
}

