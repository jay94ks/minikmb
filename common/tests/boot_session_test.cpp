#include "firmware/boot_session.hpp"
#include "firmware/native_uart_session.hpp"
#include "frame_protocol.h"
#include <cassert>
#include <deque>
#include <vector>
#include <cstdio>
int main() {
    // Boot recovery in app mode must be IDLE, not a mode-change OFF/ON cycle.
    kbm::NativeUartSession native;
    struct Source { bool pop(kbm_net_frame_t&) { return false; } } source;
    kbm_packet_t command{};
    assert(!native.next(10000,true,source,command));
    kbm::BootIdentity identities[2];
    uint8_t report[KBM_BOOT_BYTES]={1,3,1,8,1};
    assert(identities[0].observe(report,sizeof(report))==1);
    assert(identities[0].observe(report,sizeof(report))==0);
    assert(identities[1].observe(report,sizeof(report))==1); // Independent ports.
    uint8_t ack[KBM_BOOT_ACK_BYTES];identities[0].acknowledgement(ack);
    assert(kbm_boot_ack_valid(ack,sizeof(ack)) && ack[1]==1);
    assert(!kbm_boot_ack_valid(ack,sizeof(ack)-1));
    report[0]=2;assert(identities[0].observe(report,sizeof(report))==-1);
    report[0]=1;report[4]=0;assert(identities[0].observe(report,sizeof(report))==-1);
    report[4]=2;assert(identities[0].observe(report,sizeof(report))==1);
    assert(identities[1].observe(report,sizeof(report))==1);
    kbm::BootDrain drain;
    std::deque<uint16_t> queues[3]={{10,11},{20},{30}};
    drain.begin({2,1,1},true,9);
    queues[0].push_back(99); // Accepted AFTER the reset boundary, must survive.
    std::vector<uint16_t> canceled;
    bool room=false;
    auto pop=[&](unsigned queue,uint16_t& id){assert(!queues[queue].empty());id=queues[queue].front();queues[queue].pop_front();return true;};
    auto emit=[&](uint16_t id){if(!room) return false;canceled.push_back(id);return true;};
    assert(!drain.tick(pop,emit));assert(queues[0].size()==3 && canceled.empty());
    room=true;
    unsigned ticks=0;while(!drain.tick(pop,emit))assert(++ticks<20);
    assert((canceled==std::vector<uint16_t>{9,10,11,20,30}));
    assert(queues[0].size()==1&&queues[0].front()==99);
    assert(drain.tick(pop,emit));assert(canceled.size()==5);
    // Duplicate boot retry never invokes begin(); current requests remain.
    assert(identities[0].observe(report,sizeof(report))==0 && queues[0].front()==99);
    puts("PASS boot identity, retry ACK, invalid token/schema, per-port scope, bounded cancellation under USB pressure, queue boundary retains new work");
}
