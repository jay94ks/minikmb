#include "cdc_transport.h"

#include "app_state.h"
#include "master_commands.h"
#include "protocol.h"
#include "ethernet_protocol.h"
#include "native_network.h"
#include "link_protocol.h"
#include "disk_protocol.h"

#include "bsp/board_api.h"
#include "pico/stdlib.h"
#include "tusb.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint8_t data[KBM_MAX_FRAME_SIZE];
    size_t length;
} encoded_frame_t;

typedef struct {
    bool usb_ready;
    bool output_on;
    uint64_t last_toggle_us;
} status_led_t;

static void status_led_init(status_led_t *led) {
    led->usb_ready = false;
    led->output_on = false;
    led->last_toggle_us = time_us_64();
    gpio_init(PICO_DEFAULT_LED_PIN);
    gpio_set_dir(PICO_DEFAULT_LED_PIN, GPIO_OUT);
    gpio_put(PICO_DEFAULT_LED_PIN, 0);
}

static void status_led_task(status_led_t *led) {
    const bool ready = tud_mounted() && tud_ready();
    const uint64_t now = time_us_64();

    if (ready) {
        if (!led->output_on) {
            led->output_on = true;
            gpio_put(PICO_DEFAULT_LED_PIN, 1);
        }
        led->usb_ready = true;
        led->last_toggle_us = now;
        return;
    }

    if (led->usb_ready) {
        led->usb_ready = false;
        led->output_on = false;
        led->last_toggle_us = now;
        gpio_put(PICO_DEFAULT_LED_PIN, 0);
        return;
    }

    if (now - led->last_toggle_us >= 500000u) {
        led->last_toggle_us = now;
        led->output_on = !led->output_on;
        gpio_put(PICO_DEFAULT_LED_PIN, led->output_on ? 1 : 0);
    }
}

static void append_encoded_byte(uint8_t byte, void *user) {
    encoded_frame_t *frame = user;
    if (frame->length < sizeof(frame->data)) {
        frame->data[frame->length++] = byte;
    }
}

static uint8_t online_mask(void) {
    return (uint8_t)(g_online_mask & KBM_ONLINE_MASK_ALL);
}

static void put_u16(uint8_t *destination, uint32_t value) {
    destination[0] = (uint8_t)value;
    destination[1] = (uint8_t)(value >> 8u);
}

static void enqueue_local_response(uint8_t request_op,
                                   kbm_master_status_t status) {
    kbm_packet_t response = {
        .op = (uint8_t)(request_op | 0x80u),
        .len = 3,
        .payload = {(uint8_t)status, KBM_PROTOCOL_VERSION, online_mask()},
    };
    if (request_op == KBM_OP_STATUS) {
        response.len = 21;
        response.payload[19] = (uint8_t)g_serial_flags[0];
        response.payload[20] = (uint8_t)g_serial_flags[1];
        for (uint8_t port = 0; port < 2; ++port) {
            const uint8_t offset = (uint8_t)(3u + port * 8u);
            put_u16(&response.payload[offset], g_uart_diagnostics[port].tx_frames);
            put_u16(&response.payload[offset + 2u], g_uart_diagnostics[port].rx_bytes);
            put_u16(&response.payload[offset + 4u], g_uart_diagnostics[port].rx_frames);
            put_u16(&response.payload[offset + 6u], g_uart_diagnostics[port].decode_errors);
        }
    }
    queue_add_blocking(&g_usb_tx_queue, &response);
}

static void handle_packet(const kbm_packet_t *packet) {
    if (packet->op == KBM_OP_NATIVE_MODE) {
        kbm_master_status_t status = kbm_master_validate_local(packet);
        if (status == KBM_STATUS_OK && packet->len) {
            if (native_network_changing()) status = KBM_STATUS_MODE_CONFLICT;
            else native_network_request(packet->payload[0] != 0, packet->payload[1] != 0);
        }
        kbm_packet_t response;
        native_network_status((uint8_t)status, &response);
        queue_add_blocking(&g_usb_tx_queue, &response);
        return;
    }
    if (kbm_master_is_local(packet->op)) {
        const kbm_master_status_t status = kbm_master_validate_local(packet);
        enqueue_local_response(packet->op, status);
        return;
    }

    if (packet->op != KBM_OP_ROUTE) {
        enqueue_local_response(packet->op, KBM_STATUS_TARGET_REQUIRED);
        return;
    }
    kbm_bridge_request_t request = {0};
    kbm_master_status_t status = kbm_master_decode_route(packet, &request);
    if (status == KBM_STATUS_OK && native_network_owned() &&
        (request.packet.op == KBM_NET_WRITE || request.packet.op == KBM_NET_ENABLE ||
         request.packet.op == KBM_LINK_BAUD)) status = KBM_STATUS_MODE_CONFLICT;
    if (status == KBM_STATUS_OK &&
        !queue_try_add(request.packet.op == KBM_NET_WRITE || request.packet.op == KBM_DISK_DATA ? &g_uart_bulk_queue[request.port] :
            &g_uart_tx_queue[request.port], &request)) {
        status = KBM_STATUS_QUEUE_FULL;
    }
    if (status != KBM_STATUS_OK) {
        kbm_packet_t response;
        const uint8_t port = packet->len >= 1 ? packet->payload[0] : 0xff;
        const uint16_t sequence = packet->len >= 3
            ? (uint16_t)packet->payload[1] | ((uint16_t)packet->payload[2] << 8u)
            : 0;
        kbm_master_wrap_response(port, sequence, status, NULL, &response);
        queue_add_blocking(&g_usb_tx_queue, &response);
    }
}

static void handle_large_packet(const kbm_decoder_t *decoder) {
    const uint8_t *p=decoder->large;
    const uint16_t n=decoder->packet.len;
    kbm_master_status_t status=KBM_STATUS_BAD_LENGTH;
    static kbm_disk_route_t request;
    if(decoder->packet.op==KBM_OP_ROUTE && n>=4 && p[0]<2 && p[3]==KBM_DISK_DATA &&
       kbm_disk_data_valid(p+4,n-4)) {
        request.sequence=kbm_net_u16(p+1);request.transfer.len=n-4;
        memcpy(request.transfer.payload,p+4,n-4);
        if(queue_try_add(&g_uart_disk_queue[p[0]],&request)) return;
        status=KBM_STATUS_QUEUE_FULL;
    }
    kbm_packet_t response;
    kbm_master_wrap_response(n?p[0]:255,n>=3?kbm_net_u16(p+1):0,status,NULL,&response);
    queue_add_blocking(&g_usb_tx_queue,&response);
}

static void handle_decode_error(uint8_t request_op,
                                kbm_decode_result_t result) {
    kbm_master_status_t status = KBM_STATUS_BAD_ETX;
    if (result == KBM_DECODE_BAD_LENGTH) {
        status = KBM_STATUS_BAD_LENGTH;
    } else if (result == KBM_DECODE_BAD_ESCAPE) {
        status = KBM_STATUS_BAD_ESCAPE;
    } else if (result == KBM_DECODE_BAD_CHECKSUM) {
        status = KBM_STATUS_BAD_CHECKSUM;
    }
    enqueue_local_response(request_op, status);
}

static void process_cdc_rx(kbm_decoder_t *decoder) {
    uint8_t bytes[1];
    for (unsigned budget = 0; budget < 64 && tud_cdc_available() != 0 &&
         queue_get_level(&g_usb_tx_queue) < 48; ++budget) {
        const uint32_t count = tud_cdc_read(bytes, sizeof(bytes));
        for (uint32_t i = 0; i < count; ++i) {
            const kbm_decode_result_t result =
                kbm_decoder_feed(decoder, bytes[i]);
            if (result == KBM_DECODE_FRAME) {
                handle_packet(&decoder->packet);
            } else if (result == KBM_DECODE_LARGE_FRAME) {
                handle_large_packet(decoder);
            } else if (result != KBM_DECODE_MORE) {
                handle_decode_error(decoder->packet.op, result);
            }
        }
    }
}

static void process_cdc_tx(void) {
    static encoded_frame_t frame;
    static size_t offset;
    static bool active;
    static bool native_response;

    if (!active) {
        kbm_packet_t packet;
        if (!queue_try_remove(&g_usb_tx_queue, &packet) &&
            !queue_try_remove(&g_usb_bulk_queue, &packet)) {
            return;
        }
        frame.length = 0;
        native_response = packet.op == 0xf7;
        kbm_packet_write(&packet, append_encoded_byte, &frame);
        offset = 0;
        active = true;
    }

    if (!tud_ready()) {
        return;
    }
    const uint32_t available = tud_cdc_write_available();
    if (available == 0) {
        return;
    }
    size_t remaining = frame.length - offset;
    if (remaining > available) {
        remaining = available;
    }
    offset += tud_cdc_write(frame.data + offset, (uint32_t)remaining);
    tud_cdc_write_flush();
    if (offset == frame.length) {
        active = false;
        if (native_response) native_network_ack_sent();
    }
}

void cdc_transport_run(void) {
    kbm_decoder_t decoder;
    static uint8_t large_rx[KBM_MAX_LARGE_PAYLOAD];
    status_led_t status_led;
    kbm_decoder_init_large(&decoder,large_rx,sizeof(large_rx));
    tud_init(BOARD_TUD_RHPORT);
    if (board_init_after_tusb) {
        board_init_after_tusb();
    }
    status_led_init(&status_led);

    while (true) {
        tud_task();
        status_led_task(&status_led);
        process_cdc_rx(&decoder);
        process_cdc_tx();
        native_network_task();
        tight_loop_contents();
    }
}

