#include "native_network.h"
#include "app_state.h"
#include "native_dhcp.h"
#include "pico/unique_id.h"
#include "pico/stdlib.h"
#include "tusb.h"
#include "firmware/ncm_controller.hpp"
#include "firmware/pico_backends.hpp"
#include "firmware/queue.hpp"
#include "firmware/ethernet_router.hpp"
#include <string.h>

volatile uint32_t g_native_mode, g_native_dhcp, g_native_epoch, g_native_transition;
volatile uint32_t g_native_link_mask, g_native_reset_mask;
extern "C" { uint8_t tud_network_mac_address[6]; }
namespace {
struct Descriptors {
    static bool visible() { return kbm_usb_ncm_visible(); }
    static void setVisible(bool value) { kbm_usb_set_ncm_visible(value); }
};
struct Ports {
    void begin(bool mode, bool dhcp) {
        g_native_mode = mode; g_native_dhcp = dhcp;
        g_native_transition = 3;
        __atomic_add_fetch(&g_native_epoch, 1u, __ATOMIC_RELEASE);
    }
    uint32_t transitionMask() const { return __atomic_load_n(&g_native_transition, __ATOMIC_ACQUIRE); }
};
kbm::pico::Clock clock_source;
kbm::pico::UsbNcm<Descriptors> usb;
Ports ports;
kbm::NativeModeController controller(usb, clock_source, ports);
using FrameQueue = kbm::Queue<kbm_net_frame_t, 8, kbm::pico::QueueBackend>;
FrameQueue to_slave(uint8_t port) { return FrameQueue({&g_native_from_usb[port]}); }
}
static kbm_dhcp_t dhcp;
static kbm::EthernetRouter<2> router;

void native_network_init(void) {
    pico_unique_board_id_t id; pico_get_unique_board_id(&id);
    tud_network_mac_address[0]=2;
    memcpy(tud_network_mac_address+1,id.id+3,5);
    uint8_t server_mac[6]; memcpy(server_mac,tud_network_mac_address,6); server_mac[5]^=0x80;
    kbm_dhcp_init(&dhcp,server_mac);
}
bool native_network_owned(void) { return controller.owned(); }
bool native_network_changing(void) { return controller.changing(); }
void native_network_request(bool enabled, bool enable_dhcp) {
    controller.request(enabled, enable_dhcp);
}
void native_network_ack_sent(void) {
    controller.acknowledgeSent();
}
void native_network_status(uint8_t status, kbm_packet_t *p) {
    *p = {}; p->op = 0xf7; p->len = 5;
    p->payload[0] = status; p->payload[1] = controller.enabled();
    p->payload[2] = controller.dhcp(); p->payload[3] = controller.visible();
    p->payload[4] = controller.transitionMask();
}
static bool send_to(uint8_t ingress, const kbm_net_frame_t *frame) {
    if(ingress<2) return to_slave(ingress).push(*frame);
    // Local DHCP replies share the bounded USB-output queue.
    static kbm_native_frame_t reply;
    reply.port=2; reply.frame=*frame;
    return queue_try_add(&g_native_to_usb,&reply);
}
bool tud_network_recv_cb(const uint8_t *data, uint16_t length) {
    if(!g_native_mode || length<14 || length>KBM_NET_MTU) return true;
    static kbm_net_frame_t frame, reply;
    frame.length=length; memcpy(frame.data,data,length);
    if(g_native_dhcp && kbm_dhcp_process(&dhcp,2,to_ms_since_boot(get_absolute_time()),&frame,&reply))
        return !reply.length || send_to(2,&reply);
    const auto mask = router.targets(data, static_cast<uint8_t>(g_online_mask & g_native_link_mask & ~g_native_transition));
    // Sole producer: reserve all destinations before adding either copy.
    for(uint8_t i=0;i<2;i++) if((mask&(1u<<i)) && to_slave(i).full()) return false;
    for(uint8_t i=0;i<2;i++) if(mask&(1u<<i)) to_slave(i).push(frame);
    return true;
}
uint16_t tud_network_xmit_cb(uint8_t *dst, void *ref, uint16_t arg) { memcpy(dst,ref,arg); return arg; }
void tud_network_init_cb(void) {}
void native_network_task(void) {
    const uint32_t now=to_ms_since_boot(get_absolute_time());
    static kbm_native_frame_t incoming;
    static bool active;
    static uint32_t last_online;
    const uint32_t epoch = g_native_epoch;
    controller.tick();
    if(epoch != g_native_epoch) {
        memset(dhcp.leases,0,sizeof(dhcp.leases)); router.clear();
        active=false;
    }
    uint32_t online=g_online_mask;
    for(uint8_t i=0;i<2;i++) if((last_online&~online)&(1u<<i)) {
        kbm_dhcp_forget(&dhcp,i); router.forget(i);
    }
    last_online=online;
    uint32_t resets=__atomic_exchange_n(&g_native_reset_mask,0u,__ATOMIC_ACQ_REL);
    for(uint8_t i=0;i<2;i++) if(resets&(1u<<i)) kbm_dhcp_forget(&dhcp,i);
    if(!controller.ready()) {
        active=false;
        // Core 1 may still drain UART receive fragments during a USB re-enumeration.
        while(queue_try_remove(&g_native_to_usb,&incoming)) {}
        return;
    }
    tud_network_recv_renew();
    if(!active) active=queue_try_remove(&g_native_to_usb,&incoming);
    if(!active) return;
    if(incoming.port<2) {
        router.learn(incoming.port,incoming.frame.data+6);
        static kbm_net_frame_t reply;
        if(g_native_dhcp && kbm_dhcp_process(&dhcp,incoming.port,now,&incoming.frame,&reply)) {
            if(!reply.length || send_to(incoming.port,&reply)) active=false;
            return;
        }
    }
    // No slave-to-slave forwarding. Only USB host sees non-DHCP slave traffic.
    if(tud_network_can_xmit(incoming.frame.length)) {
        tud_network_xmit(incoming.frame.data,incoming.frame.length); active=false;
    }
}
