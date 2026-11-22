#include "network_bridge.h"
#include "disk_bridge.h"
#include "app_state.h"
#include "ethernet_protocol.h"
#include "protocol.h"
#include "usb_descriptors.h"
#include "pico/unique_id.h"
#include "pico/stdlib.h"
#include "tusb.h"
#include "firmware/ncm_controller.hpp"
#include "firmware/pico_backends.hpp"
#include "firmware/queue.hpp"
#include "firmware/fragment_sender.hpp"
#include <string.h>

extern "C" { uint8_t tud_network_mac_address[6]; }
namespace {
struct Descriptors {
    static uint8_t visible() { return kbm_usb_function_mask(); }
    static void setVisible(uint8_t value) { kbm_usb_set_function_mask(value); }
};
kbm::pico::Clock clock_source;
struct UsbFunctions {
    uint8_t visible() const { return Descriptors::visible(); }
    void setVisible(uint8_t value) { Descriptors::setVisible(value); }
    void disconnect() { tud_disconnect(); }
    void connect() { tud_connect(); }
    bool ready() const { return tud_ready() && kbm_ncm_link_ready(); }
} usb;
kbm::NcmVisibility<UsbFunctions, kbm::pico::Clock, uint8_t> visibility(usb, clock_source);
using FrameQueue = kbm::Queue<kbm_net_frame_t, 8, kbm::pico::QueueBackend>;
}

void network_bridge_init(void) {
    pico_unique_board_id_t id;
    pico_get_unique_board_id(&id);
    tud_network_mac_address[0] = 0x02; // locally administered, stable per board
    memcpy(tud_network_mac_address + 1, id.id + 3, 5);
}
bool network_bridge_usb_available(void) { return !visibility.changing(); }

bool tud_network_recv_cb(const uint8_t *data, uint16_t length) {
    if (!__atomic_load_n(&g_net_enabled, __ATOMIC_ACQUIRE)) return true;
    if (length < 14 || length > KBM_NET_MTU) return true;
    static kbm_net_frame_t frame;
    frame.length = length;
    memcpy(frame.data, data, length);
    // false leaves the datagram owned by TinyUSB until recv_renew retries it.
    return FrameQueue({&g_net_rx_queue}).push(frame);
}
uint16_t tud_network_xmit_cb(uint8_t *dst, void *ref, uint16_t arg) {
    memcpy(dst, ref, arg);
    return arg;
}
void tud_network_init_cb(void) {}

void network_bridge_task(void) {
    static kbm_net_frame_t tx;
    static bool tx_active, sent;
    static kbm::FragmentSender sender;
    static uint32_t last_send;
    const bool enabled = __atomic_load_n(&g_net_enabled, __ATOMIC_ACQUIRE) != 0;
    const uint32_t now = to_ms_since_boot(get_absolute_time());
    const uint8_t functions = (enabled ? KBM_USB_NCM : 0) |
                              (disk_bridge_visible() ? KBM_USB_DISK : 0);
    visibility.request(functions);
    visibility.tick();
    const bool ready = enabled && visibility.ready();
    __atomic_store_n(&g_net_ready, ready, __ATOMIC_RELEASE);
    if (!ready || !enabled) {
        tx_active = sent = false;
        sender.reset();
        static kbm_net_frame_t discarded;
        while (queue_try_remove(&g_net_rx_queue, &discarded)) {}
        while (queue_try_remove(&g_net_tx_queue, &discarded)) {}
        kbm_packet_t event;
        while (queue_try_remove(&g_net_event_queue, &event)) {}
        return;
    }
    tud_network_recv_renew();
    if (!tx_active && FrameQueue({&g_net_tx_queue}).pop(tx)) tx_active = true;
    if (tx_active && tud_network_can_xmit(tx.length)) {
        tud_network_xmit(tx.data, tx.length);
        tx_active = false;
    }
    if (sent && __atomic_load_n(&g_net_credit, __ATOMIC_ACQUIRE) == sender.key()) {
        sender.acknowledge(true);
        sent = false;
    }
    if (sent && (uint32_t)(now - last_send) < 500u) return;
    kbm_packet_t event{};
    FrameQueue source({&g_net_rx_queue});
    if (!sender.next(source, event)) return;
    event.op = KBM_NET_EVENT;
    if (!sent) __atomic_store_n(&g_net_credit, ~sender.key(), __ATOMIC_RELEASE);
    if (queue_try_add(&g_net_event_queue, &event)) {
        sent = true; last_send = now;
    }
}
