#include "uart_bridge.h"

#include "app_state.h"
#include "master_commands.h"
#include "protocol.h"
#include "response_tracker.h"
#include "ethernet_protocol.h"
#include "link_protocol.h"
#include "native_network.h"
#include "reboot_protocol.h"
#include "disk_protocol.h"
#include "firmware/native_uart_session.hpp"
#include "firmware/pico_backends.hpp"
#include "firmware/queue.hpp"
#include "firmware/uart_io.hpp"
#include "uart_dma_transport.h"
#include "firmware/boot_session.hpp"
#include "hid_diagnostics.h"

#include "hardware/gpio.h"
#include "hardware/uart.h"
#include "pico/stdlib.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#define KBM_PORT_COUNT 2u
#define KBM_PING_INTERVAL_MS 100u
#define KBM_RESPONSE_TIMEOUT_MS 500u

typedef struct {
    uart_inst_t *uart;
    kbm::pico::Uart device;
    kbm_decoder_t decoder;
    kbm_uart_dma_t *dma;
    bool tx_dma_started;
    kbm_response_tracker_t responses;
    uint32_t last_tx_ms;
    bool host_pending;
    bool recovering;
    uint16_t host_sequence;
    uint8_t tx_bytes[KBM_MAX_LARGE_FRAME_SIZE];
    size_t tx_length;
    size_t tx_offset;
    uint8_t tx_request_op;
    bool tx_forward;
    bool tx_track;
    uint32_t last_serial_probe_ms;
    kbm_packet_t serial_event;
    bool serial_event_pending;
    bool serial_ack_pending;
    uint16_t serial_ack_sequence;
    uint16_t last_serial_sequence;
    bool serial_sequence_valid;
    kbm_packet_t net_event;
    bool net_pending, net_ack_pending, net_seen;
    uint32_t net_key;
    uint32_t baud_candidate, baud_switch_ms, baud_restore_ms;
    uint16_t baud_lease;
    uint32_t native_epoch;
    bool native_complete;
    kbm::NativeUartSession native;
    kbm_net_assembly_t native_assembly;
    kbm::BootIdentity boot_identity;
    kbm::BootDrain boot_drain;
    kbm_packet_t boot_event;
    bool boot_resetting,boot_event_pending,boot_ack_pending;
} uart_port_t;

static uart_port_t ports[KBM_PORT_COUNT];

static void write_uart_byte(uint8_t byte, void *user) {
    uart_port_t *port = static_cast<uart_port_t*>(user);
    port->tx_bytes[port->tx_length++] = byte;
}

static bool pending_full(const uart_port_t *port) {
    return kbm_response_tracker_full(&port->responses);
}

static void set_serial_flags(uint8_t index, uint8_t flags) {
    if ((uint8_t)g_serial_flags[index] == flags) return;
    g_serial_flags[index] = flags;
    if (!(flags & KBM_SERIAL_FLAG_USB)) ports[index].serial_sequence_valid = false;
    const kbm_packet_t event = {
        .op = KBM_SERIAL_EVENT_STATE, .len = 2, .payload = {index, flags},
    };
    queue_add_blocking(&g_usb_tx_queue, &event);
}

static void set_online(uint8_t index, bool online) {
    const uint32_t bit = 1u << index;
    const bool was_online = (g_online_mask & bit) != 0;
    if (was_online == online) {
        return;
    }
    if (online) {
        __atomic_fetch_or(&g_online_mask, bit, __ATOMIC_RELEASE);
    } else {
        __atomic_fetch_and(&g_online_mask, ~bit, __ATOMIC_RELEASE);
    }

    const kbm_packet_t event = {
        .op = KBM_EVENT_ONLINE_CHANGED,
        .len = 2,
        .payload = {index, static_cast<uint8_t>(online)},
    };
    queue_add_blocking(&g_usb_tx_queue, &event);
}

static void transmit_frame(uint8_t index,uint8_t op,uint16_t len,const uint8_t *payload,
                     bool forward_response) {
    uart_port_t *port = &ports[index];
    port->tx_length = 0;
    port->tx_offset = 0;
    port->tx_dma_started = false;
    port->tx_request_op = op;
    port->tx_forward = forward_response;
    port->tx_track = op != KBM_SERIAL_OP_RX_ACK && op != KBM_NET_CREDIT && op!=KBM_BOOT_ACK;
    kbm_frame_write(op,len,payload,write_uart_byte,port);
}
static void transmit(uint8_t index,const kbm_packet_t *packet,bool forward) {
    transmit_frame(index,packet->op,packet->len,packet->payload,forward);
}

static void service_uart_tx(uint8_t index) {
    uart_port_t *port = &ports[index];
    if (port->tx_length == 0) return;
    if(!port->tx_dma_started) {
        kbm_uart_dma_tx_start(port->dma,port->tx_bytes,port->tx_length);
        port->tx_dma_started=true;
    }
    if (!kbm_uart_dma_tx_busy(port->dma)) {
        const uint32_t sent_ms = to_ms_since_boot(get_absolute_time());
        port->last_tx_ms = sent_ms;
        g_uart_diagnostics[index].tx_frames = g_uart_diagnostics[index].tx_frames + 1u;
        if (port->tx_track) {
            (void)kbm_response_tracker_push(&port->responses, sent_ms,
                                            port->tx_request_op, port->tx_forward);
        }
        port->tx_length = 0;
        port->tx_offset = 0;
    }
}

static void service_serial_rx(uint8_t index) {
    uart_port_t *port = &ports[index];
    if (!port->serial_event_pending) return;
    const kbm_packet_t *source = &port->serial_event;
    kbm_packet_t event = {.op = KBM_SERIAL_EVENT_RX,
                         .len = (uint8_t)(source->len + 1u), .payload = {0}};
    event.payload[0] = index;
    memcpy(&event.payload[1], source->payload, source->len);
    if (queue_try_add(&g_usb_tx_queue, &event)) {
        port->serial_event_pending = false;
        port->last_serial_sequence = (uint16_t)source->payload[0] |
            ((uint16_t)source->payload[1] << 8u);
        port->serial_sequence_valid = true;
        port->serial_ack_sequence = port->last_serial_sequence;
        port->serial_ack_pending = true;
    }
}

static void publish_native(uint8_t index) {
    auto& session = ports[index].native;
    const uint32_t bit = 1u << index;
    if (session.ready()) __atomic_fetch_or(&g_native_link_mask, bit, __ATOMIC_RELEASE);
    else __atomic_fetch_and(&g_native_link_mask, ~bit, __ATOMIC_RELEASE);
    if (!session.transitioning()) __atomic_fetch_and(&g_native_transition, ~bit, __ATOMIC_RELEASE);
    if (session.takeReset()) __atomic_fetch_or(&g_native_reset_mask, bit, __ATOMIC_RELEASE);
}

static void handle_uart_frame(uint8_t index, const kbm_packet_t *packet) {
    kbm_pending_request_t pending;
    uart_port_t *port = &ports[index];
    if(packet->op==KBM_BOOT_EVENT) {
        const int boot=port->boot_identity.observe(packet->payload,packet->len);
        if(boot<0) return;
        port->boot_ack_pending=true;
        if(boot==0) return; // Lost BOOT_ACK retry: never erase new work twice.
        port->boot_event={.op=KBM_BOOT_EVENT,.len=KBM_BOOT_BYTES+1,.payload={index}};
        memcpy(port->boot_event.payload+1,packet->payload,KBM_BOOT_BYTES);
        port->boot_event_pending=port->boot_resetting=true;
        port->boot_drain.begin({(uint16_t)queue_get_level(&g_uart_tx_queue[index]),
            (uint16_t)queue_get_level(&g_uart_bulk_queue[index]),
            (uint16_t)queue_get_level(&g_uart_disk_queue[index])},port->host_pending,port->host_sequence);
        port->host_pending=false;port->recovering=true;
        port->baud_candidate=port->baud_switch_ms=port->baud_restore_ms=0;
        // DMA owns tx_bytes until completion. Finish old bytes, but don't track
        // or reuse their ACK in the new session; BOOT_ACK follows UART BUSY=0.
        port->tx_track=port->tx_forward=false;
        kbm_response_tracker_clear(&port->responses);
        port->serial_event_pending=port->serial_ack_pending=port->serial_sequence_valid=false;
        port->net_pending=port->net_ack_pending=port->net_seen=port->native_complete=false;
        memset(&port->native_assembly,0,sizeof(port->native_assembly));
        port->native=kbm::NativeUartSession{};
        if(g_native_mode) port->native.begin(true,to_ms_since_boot(get_absolute_time()));
        __atomic_fetch_or(&g_native_reset_mask,1u<<index,__ATOMIC_RELEASE);
        __atomic_fetch_and(&g_native_link_mask,~(1u<<index),__ATOMIC_RELEASE);
        static kbm_net_frame_t discarded;
        while(queue_try_remove(&g_native_from_usb[index],&discarded)) {}
        // BOOT event must reach USB BEFORE canceled responses can pump new work.
        // Online/serial flags are sampled immediately; notification is event F9.
        __atomic_fetch_and(&g_online_mask,~(1u<<index),__ATOMIC_RELEASE);
        g_serial_flags[index]=0;
        return;
    }
    if(port->boot_resetting) return; // Old-session ACKs/events are not new traffic.
    set_online(index, true);
    if(packet->op==KBM_HID_DIAGNOSTICS_EVENT && kbm_hid_diagnostics_valid(packet->payload,packet->len)) {
        kbm_packet_t event={.op=KBM_HID_DIAGNOSTICS_EVENT,.len=KBM_HID_DIAGNOSTICS_BYTES+1,.payload={index}};
        memcpy(event.payload+1,packet->payload,packet->len);
        (void)queue_try_add(&g_usb_tx_queue,&event); // Periodic snapshot recovers loss.
        return; // Telemetry must never consume a command ACK.
    }
    if(packet->op==KBM_DISK_READ_EVENT && packet->len==14) {
        kbm_packet_t event{}; event.op=KBM_DISK_READ_EVENT; event.len=15;
        event.payload[0]=index; memcpy(event.payload+1,packet->payload,14);
        // No credit/ACK consumption: slave retries the same token after 500ms.
        queue_try_add(&g_usb_bulk_queue,&event);
        return;
    }
    if (packet->op == KBM_NET_EVENT && packet->len > KBM_NET_HEADER &&
        packet->len <= KBM_NET_HEADER + KBM_NET_CHUNK) {
        uint32_t key = (uint32_t)kbm_net_u16(packet->payload) << 16 | kbm_net_u16(packet->payload + 4);
        if (port->net_seen && port->net_key == key) port->net_ack_pending = true;
        else if (!port->net_pending) { port->net_event = *packet; port->net_pending = true; }
        return;
    }
    if (packet->op == KBM_SERIAL_EVENT_STATE && packet->len == 1) {
        set_serial_flags(index, packet->payload[0]);
        return;
    }
    if (packet->op == KBM_SERIAL_EVENT_RX && packet->len >= 3 && packet->len <= 250) {
        const uint16_t sequence = (uint16_t)packet->payload[0] |
            ((uint16_t)packet->payload[1] << 8u);
        if (port->serial_sequence_valid && sequence == port->last_serial_sequence) {
            port->serial_ack_sequence = sequence;
            port->serial_ack_pending = true;
        } else if (!port->serial_event_pending) {
            port->serial_event = *packet;
            port->serial_event_pending = true;
        }
        return; // Async serial data must never consume a HID/command ACK.
    }
    if (packet->op == (KBM_SERIAL_OP_STATUS | 0x80u) &&
        packet->len == 2 && packet->payload[0] == 0) {
        set_serial_flags(index, packet->payload[1]);
    }
    if (kbm_response_tracker_match(&ports[index].responses, packet->op,
                                   &pending)) {
        if (!pending.forward_response) {
            if (port->native.acknowledge(*packet, to_ms_since_boot(get_absolute_time()))) {
                publish_native(index);
                return;
            }
            // A newly tracked PING ACK forms an ordering barrier after timeout.
            if (packet->op == (KBM_OP_PING | 0x80u)) ports[index].recovering = false;
            return;
        }
        if (ports[index].host_pending) {
            if (packet->op == (KBM_REBOOT_OP | 0x80u) && packet->len && packet->payload[0] == 0) {
                __atomic_fetch_or(&g_native_reset_mask, 1u << index, __ATOMIC_RELEASE);
                __atomic_fetch_and(&g_native_link_mask, ~(1u << index), __ATOMIC_RELEASE);
                port->native.rebooted(to_ms_since_boot(get_absolute_time()));
                publish_native(index);
            }
            if (packet->op == (KBM_LINK_BAUD | 0x80u) && packet->payload[0] == 0 && port->baud_candidate) {
                port->baud_switch_ms = to_ms_since_boot(get_absolute_time()) + 30u;
            }
            kbm_packet_t response;
            kbm_master_wrap_response(index, ports[index].host_sequence,
                                     KBM_STATUS_OK, packet, &response);
            ports[index].host_pending = false;
            queue_add_blocking(&g_usb_tx_queue, &response);
        }
    }
}

static bool service_boot(uint8_t index) {
    uart_port_t *port=&ports[index];
    if(port->boot_resetting) {
        if(port->boot_event_pending) {
            if(!queue_try_add(&g_usb_tx_queue,&port->boot_event)) return true;
            port->boot_event_pending=false;
        }
        const bool drained=port->boot_drain.tick([index](unsigned queue,uint16_t& sequence) {
            if(queue==2) {
                static kbm_disk_route_t request;
                if(!queue_try_remove(&g_uart_disk_queue[index],&request)) return false;
                sequence=request.sequence;return true;
            }
            kbm_bridge_request_t request;
            if(!queue_try_remove(queue==0?&g_uart_tx_queue[index]:&g_uart_bulk_queue[index],&request)) return false;
            sequence=request.sequence;return true;
        },[index](uint16_t sequence) {
            kbm_packet_t response;
            kbm_master_wrap_response(index,sequence,KBM_STATUS_SLAVE_RESTARTED,nullptr,&response);
            return queue_try_add(&g_usb_tx_queue,&response);
        });
        if(!drained) return true;
    }
    if(!port->boot_ack_pending) return false;
    if(port->tx_length || kbm_uart_dma_tx_busy(port->dma) ||
       (uart_get_hw(port->uart)->fr&UART_UARTFR_BUSY_BITS)) return true;
    kbm_packet_t ack={.op=KBM_BOOT_ACK,.len=KBM_BOOT_ACK_BYTES,.payload={0}};
    port->device.setBaud(KBM_UART_BAUD);
    port->boot_identity.acknowledgement(ack.payload);
    transmit(index,&ack,false);
    port->boot_ack_pending=port->boot_resetting=false;
    port->last_serial_probe_ms=0;
    return true;
}

static void poll_uart(uint8_t index) {
    uart_port_t *port = &ports[index];
    uint8_t byte;
    for(unsigned budget=0;budget<64;++budget) {
        const int received=kbm_uart_dma_read(port->dma,&byte);
        if(!received) break;
        if(received<0) {
            g_uart_diagnostics[index].decode_errors=g_uart_diagnostics[index].decode_errors+1u;
            kbm_decoder_init(&port->decoder);continue;
        }
        g_uart_diagnostics[index].rx_bytes = g_uart_diagnostics[index].rx_bytes + 1u;
        const kbm_decode_result_t result =
            kbm_decoder_feed(&port->decoder, byte);
        if (result == KBM_DECODE_FRAME) {
            g_uart_diagnostics[index].rx_frames = g_uart_diagnostics[index].rx_frames + 1u;
            handle_uart_frame(index, &port->decoder.packet);
        } else if (result != KBM_DECODE_MORE) {
            g_uart_diagnostics[index].decode_errors = g_uart_diagnostics[index].decode_errors + 1u;
        }
    }
}

static void check_timeout(uint8_t index, uint32_t now_ms) {
    uart_port_t *port = &ports[index];
    const kbm_pending_request_t *oldest =
        kbm_response_tracker_oldest(&port->responses);
    if (oldest == NULL) {
        return;
    }
    if ((uint32_t)(now_ms - oldest->sent_ms) >= KBM_RESPONSE_TIMEOUT_MS) {
        set_online(index, false);
        set_serial_flags(index, 0);
        port->net_seen = false;
        port->net_pending = port->net_ack_pending = false;
        if (port->native.pending()) {
            port->native.timedOut();
            port->recovering = true;
        }
        __atomic_fetch_and(&g_native_transition, ~(1u << index), __ATOMIC_RELEASE);
        __atomic_fetch_and(&g_native_link_mask, ~(1u << index), __ATOMIC_RELEASE);
        if (port->host_pending) {
            kbm_packet_t response;
            kbm_master_wrap_response(index, port->host_sequence,
                                     KBM_STATUS_UART_TIMEOUT, NULL, &response);
            port->host_pending = false;
            port->recovering = true;
            queue_add_blocking(&g_usb_tx_queue, &response);
        }
        kbm_response_tracker_clear(&port->responses);
    }
}

// Core 1 owns all native UART state. Each internal request waits for its real ACK.
static bool service_native(uint8_t index, uint32_t now) {
    uart_port_t *p = &ports[index];
    if (p->native.pending() || p->host_pending || p->recovering ||
        kbm_response_tracker_oldest(&p->responses)) return false;
    kbm_packet_t packet{};
    kbm::Queue<kbm_net_frame_t, 8, kbm::pico::QueueBackend> source({&g_native_from_usb[index]});
    const bool ready = p->native.next(now, (g_online_mask & (1u << index)) != 0, source, packet);
    publish_native(index);
    if (ready) transmit(index, &packet, false);
    return ready;
}

static void native_epoch_task(uint8_t index, uint32_t now) {
    uart_port_t *p = &ports[index];
    const uint32_t epoch = __atomic_load_n(&g_native_epoch, __ATOMIC_ACQUIRE);
    if (p->native_epoch == epoch || p->host_pending || p->native.pending() || p->tx_length) return;
    p->native_epoch = epoch;
    p->native.begin(g_native_mode != 0, now);
    __atomic_fetch_and(&g_native_link_mask, ~(1u << index), __ATOMIC_RELEASE);
    p->native_complete = false;
    p->net_pending = p->net_seen = p->net_ack_pending = false;
    memset(&p->native_assembly, 0, sizeof(p->native_assembly));
    static kbm_net_frame_t discarded;
    while (queue_try_remove(&g_native_from_usb[index], &discarded)) {}
}

static void init_uart_port(uint8_t index, uart_inst_t *uart, uint tx_pin,
                           uint rx_pin, gpio_function_t function) {
    ports[index].uart = uart;
    ports[index].device.instance = uart;
    kbm_response_tracker_init(&ports[index].responses);
    ports[index].last_tx_ms = to_ms_since_boot(get_absolute_time());
    ports[index].host_pending = false;
    ports[index].recovering = false;
    kbm_decoder_init(&ports[index].decoder);
    uart_init(uart, KBM_UART_BAUD);
    uart_set_format(uart, 8, 1, UART_PARITY_NONE);
    uart_set_hw_flow(uart, false, false);
    uart_set_fifo_enabled(uart, true);
    gpio_set_function(tx_pin, function);
    gpio_set_function(rx_pin, function);
    ports[index].dma=kbm_uart_dma_init(uart);
}

void uart_bridge_core1(void) {
    init_uart_port(0, uart0, KBM_UART0_TX_PIN, KBM_UART0_RX_PIN,
                   GPIO_FUNC_UART);
    init_uart_port(1, uart1, KBM_UART1_TX_PIN, KBM_UART1_RX_PIN,
                   GPIO_FUNC_UART);

    const kbm_packet_t ping = {.op = KBM_OP_PING, .len = 0, .payload = {0}};
    const kbm_packet_t serial_status = {.op = KBM_SERIAL_OP_STATUS, .len = 0, .payload = {0}};

    while (true) {
        // Register completed DMA TX before consuming a possibly buffered ACK.
        service_uart_tx(0);
        service_uart_tx(1);
        poll_uart(0);
        poll_uart(1);
        service_serial_rx(0);
        service_serial_rx(1);

        const uint32_t now_ms = to_ms_since_boot(get_absolute_time());
        check_timeout(0, now_ms);
        check_timeout(1, now_ms);

        for (uint8_t index = 0; index < KBM_PORT_COUNT; ++index) {
            uart_port_t *port = &ports[index];
            if(service_boot(index)) continue;
            const uint32_t baud_now = to_ms_since_boot(get_absolute_time());
            native_epoch_task(index, baud_now);
            if (port->baud_switch_ms) {
                if ((int32_t)(baud_now - port->baud_switch_ms) < 0) continue;
                port->device.setBaud(port->baud_candidate);
                port->baud_candidate = port->baud_switch_ms = 0;
                port->baud_restore_ms = baud_now + port->baud_lease;
                kbm_decoder_init(&port->decoder);
                kbm_uart_dma_discard(port->dma);
                kbm_response_tracker_clear(&port->responses);
            }
            if (port->baud_restore_ms && (int32_t)(baud_now - port->baud_restore_ms) >= 0) {
                port->device.setBaud(KBM_UART_BAUD);
                port->baud_restore_ms = 0;
                kbm_decoder_init(&port->decoder);
                kbm_uart_dma_discard(port->dma);
                kbm_response_tracker_clear(&port->responses);
            }
            if (ports[index].tx_length != 0) continue;
            if (ports[index].serial_ack_pending) {
                const uint16_t sequence = ports[index].serial_ack_sequence;
                const kbm_packet_t ack = {.op = KBM_SERIAL_OP_RX_ACK, .len = 2,
                    .payload = {(uint8_t)sequence, (uint8_t)(sequence >> 8u)}};
                transmit(index, &ack, false);
                ports[index].serial_ack_pending = false;
                continue;
            }
            kbm_bridge_request_t request;
            if (!ports[index].host_pending && !ports[index].native.pending() && !ports[index].recovering &&
                !pending_full(&ports[index]) &&
                queue_try_remove(&g_uart_tx_queue[index], &request)) {
                if ((g_native_mode || g_native_transition) &&
                    (request.packet.op == KBM_NET_ENABLE || request.packet.op == KBM_LINK_BAUD)) {
                    kbm_packet_t response;
                    kbm_master_wrap_response(index, request.sequence, KBM_STATUS_MODE_CONFLICT, NULL, &response);
                    queue_add_blocking(&g_usb_tx_queue, &response);
                    continue;
                }
                ports[index].host_sequence = request.sequence;
                ports[index].host_pending = true;
                if (request.packet.op == KBM_LINK_BAUD && request.packet.len == 6 &&
                    kbm_link_lease_valid(request.packet.payload)) {
                    port->baud_candidate = kbm_net_u32(request.packet.payload);
                    port->baud_lease = kbm_net_u16(request.packet.payload + 4);
                }
                transmit(index, &request.packet, true);
                continue;
            }
            // Forward one small Ethernet fragment only after high priority work.
            if (ports[index].net_pending) {
                const kbm_packet_t *source = &ports[index].net_event;
                if (g_native_mode) {
                    int result = 1;
                    if (!port->native_complete) result = kbm_net_accept(&port->native_assembly, source->payload, source->len);
                    bool accepted = result == 0;
                    if (result == 1) {
                        port->native_complete = true;
                        static kbm_native_frame_t incoming;
                        incoming.port = index; incoming.frame = port->native_assembly.frame;
                        accepted = queue_try_add(&g_native_to_usb, &incoming);
                    }
                    if (result < 0) { // Drop corrupt frame, credit to let the sender recover on its next frame.
                        accepted = true; port->native_assembly.offset = 0;
                    }
                    if (accepted) {
                        port->native_complete = false;
                        port->net_key = (uint32_t)kbm_net_u16(source->payload) << 16 | kbm_net_u16(source->payload + 4);
                        port->net_seen = port->net_ack_pending = true; port->net_pending = false;
                    }
                } else {
                kbm_packet_t event = {.op = KBM_NET_EVENT, .len = (uint8_t)(source->len + 1), .payload = {0}};
                event.payload[0] = index;
                memcpy(event.payload + 1, source->payload, source->len);
                if (queue_try_add(&g_usb_bulk_queue, &event)) {
                    ports[index].net_key = (uint32_t)kbm_net_u16(source->payload) << 16 |
                        kbm_net_u16(source->payload + 4);
                    ports[index].net_seen = true;
                    ports[index].net_pending = false;
                    ports[index].net_ack_pending = true;
                }
                }
            }
            if (ports[index].net_ack_pending) {
                const uint32_t key = ports[index].net_key;
                kbm_packet_t credit = {.op = KBM_NET_CREDIT, .len = 4, .payload = {0}};
                kbm_net_put16(credit.payload, key >> 16);
                kbm_net_put16(credit.payload + 2, key);
                transmit(index, &credit, false);
                ports[index].net_ack_pending = false;
                continue;
            }
            if (!ports[index].host_pending && !ports[index].native.pending() && !ports[index].recovering &&
                !pending_full(&ports[index]) &&
                queue_try_remove(&g_uart_bulk_queue[index], &request)) {
                if ((g_native_mode || g_native_transition) && request.packet.op==KBM_NET_WRITE) {
                    kbm_packet_t response;
                    kbm_master_wrap_response(index, request.sequence, KBM_STATUS_MODE_CONFLICT, NULL, &response);
                    queue_add_blocking(&g_usb_tx_queue, &response);
                    continue;
                }
                ports[index].host_sequence = request.sequence;
                ports[index].host_pending = true;
                transmit(index, &request.packet, true);
                continue;
            }
            // Large FS frames remain low priority, with one bounded4KiB slot
            // per UART instead of multiplying all normal queue elements.
            static kbm_disk_route_t disk_request;
            if (!port->host_pending && !port->native.pending() && !port->recovering &&
                !pending_full(port) && queue_try_remove(&g_uart_disk_queue[index],&disk_request)) {
                port->host_sequence=disk_request.sequence;port->host_pending=true;
                transmit_frame(index,KBM_DISK_DATA,disk_request.transfer.len,disk_request.transfer.payload,true);
                continue;
            }
            if (service_native(index, baud_now)) continue;
            // transmit() may have advanced time past now_ms. Sample again to
            // avoid unsigned subtraction scheduling a premature keepalive.
            const uint32_t ping_now_ms = to_ms_since_boot(get_absolute_time());
            if ((uint32_t)(ping_now_ms - ports[index].last_tx_ms) >=
                    KBM_PING_INTERVAL_MS &&
                !pending_full(&ports[index])) {
                if (port->native.pending()) continue;
                transmit(index, &ping, false);
                continue;
            }
            if ((uint32_t)(ping_now_ms - ports[index].last_serial_probe_ms) >= 1000u &&
                !ports[index].host_pending && !ports[index].recovering &&
                !ports[index].native.pending() &&
                !pending_full(&ports[index])) {
                ports[index].last_serial_probe_ms = ping_now_ms;
                transmit(index, &serial_status, false);
            }
        }
        tight_loop_contents();
    }
}

