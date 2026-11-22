#include "firmware/ncm_controller.hpp"
#include "firmware/native_uart_session.hpp"
#include "firmware/queue.hpp"
#include "firmware/uart_io.hpp"
#include "firmware/ethernet_router.hpp"
#include "firmware/disk_reader.hpp"
#include "mock_backends.hpp"
#include "protocol.h"
#include "native_dhcp.h"
#include "reboot_guard.h"
#include <cstdio>
#include <type_traits>

static_assert(!std::is_polymorphic_v<kbm::NativeUartSession>);
static_assert(sizeof(kbm::FragmentSender)<1600);
using FrameStore=mock::Storage<kbm_net_frame_t,8>;
using FrameQueue=kbm::Queue<kbm_net_frame_t,8,FrameStore::Backend>;

void test_mode() {
    mock::Clock clock; mock::Usb usb; mock::Ports ports;
    kbm::NativeModeController mode(usb,clock,ports);
    assert(!mode.owned() && !mode.ready());
    assert(mode.request(true,false)); assert(mode.owned());
    clock.advance(1000); mode.tick(); assert(ports.begins==0 && usb.disconnects==0);
    mode.acknowledgeSent(); clock.advance(199); mode.tick(); assert(ports.begins==0);
    clock.advance(1); mode.tick(); assert(ports.begins==1 && ports.mode && !ports.dhcp);
    assert(usb.visible() && usb.disconnects==1 && !mode.ready());
    assert(!mode.request(false,true)); // No overlap/stale mode ACK can start another transition.
    mode.acknowledgeSent(); clock.advance(299); mode.tick(); assert(usb.connects==0);
    clock.advance(1); mode.tick(); assert(usb.connects==1);
    ports.mask=0; usb.configured=true; assert(mode.ready());
    assert(mode.request(true,true)); mode.acknowledgeSent(); clock.advance(200); mode.tick();
    assert(ports.begins==2 && ports.dhcp && usb.disconnects==1); // DHCP argument cycles both slaves too.
    ports.mask=0;
    assert(mode.request(false,false)); mode.acknowledgeSent(); clock.advance(500); mode.tick();
    assert(!ports.mode && !ports.dhcp && !usb.visible() && usb.disconnects==2);
    ports.mask=0; clock.advance(300); mode.tick(); assert(!mode.owned());
    clock.value=0xfffffff0u;
    assert(mode.request(true,true)); mode.acknowledgeSent(); clock.advance(200); mode.tick();
    assert(ports.mode); clock.advance(300); mode.tick(); assert(usb.connects==3);
    std::puts("PASS native mode ACK-before-detach, DHCP parameter, OFF/ON, wraparound, no overlap");
}
void test_optional_functions() {
    struct Functions : mock::Usb {
        uint8_t mask{};
        uint8_t visible() const { return mask; }
        void setVisible(uint8_t value) { assert(detached); mask=value; ++descriptor_changes; }
    } usb;
    mock::Clock clock;
    kbm::NcmVisibility<Functions,mock::Clock,uint8_t> visibility(usb,clock);
    visibility.request(0); visibility.tick();
    assert(!visibility.changing() && !usb.disconnects && !usb.visible());
    visibility.request(2); // Explicit folder-share ON exposes MSC, no NCM.
    assert(usb.visible()==2 && usb.detached && visibility.changing());
    clock.advance(200); visibility.request(3); // NCM changes during the same detach.
    clock.advance(299); visibility.tick(); assert(!usb.connects);
    clock.advance(1); visibility.tick(); assert(usb.connects==1 && usb.visible()==3);
    usb.configured=true; assert(visibility.ready());
    visibility.request(1); // Disk OFF/lease expiry preserves NCM.
    clock.advance(300); visibility.tick(); assert(usb.visible()==1 && usb.connects==2);
    visibility.request(0); clock.advance(300); visibility.tick();
    assert(!usb.visible() && !visibility.ready() && usb.connects==3);
    clock.value=0xfffffff0u; visibility.request(2);
    clock.advance(300); visibility.tick(); assert(usb.connects==4);
    std::puts("PASS composite USB hidden boot, MSC/NCM independent masks, shared detach, OFF, wraparound");
}
void activate(kbm::NativeUartSession& link, FrameQueue& queue, std::uint32_t now, bool mode=true) {
    kbm_packet_t p{};
    link.begin(mode,now);
    assert(link.next(now,true,queue,p) && p.op==0x43 && p.payload[0]==0);
    assert(!link.next(now+100,true,queue,p)); // Lost/late ACK: no next packet.
    kbm_packet_t wrong{}; wrong.op=0x80; wrong.len=2;
    assert(!link.acknowledge(wrong,now));
    p.op=0xc3; p.len=2; p.payload[0]=0;
    assert(link.acknowledge(p,now));
    assert(!link.next(now+599,true,queue,p));
    assert(link.next(now+600,true,queue,p) && p.op==0x43 && p.payload[0]==1);
    p.op=0xc3; p.len=2; p.payload[0]=0; assert(link.acknowledge(p,now+600));
    assert(!link.transitioning());
    if(!mode) { assert(!link.next(now+5000,true,queue,p)); return; }
    assert(link.next(now+1200,true,queue,p) && p.op==0x41);
    p.op=0xc1; p.len=2; p.payload[0]=0; p.payload[1]=3;
    assert(link.acknowledge(p,now+1200) && link.ready());
}
void test_links() {
    FrameStore storage; FrameQueue queue({&storage});
    kbm::NativeUartSession a,b;
    activate(a,queue,0); activate(b,queue,0);
    kbm_net_frame_t frame{}; frame.length=1514;
    for(std::uint16_t i=0;i<frame.length;i++) frame.data[i]=static_cast<std::uint8_t>(i*47);
    for(unsigned n=0;n<8;n++) assert(queue.push(frame));
    assert(queue.full() && !queue.push(frame));
    kbm_packet_t p{}; kbm_net_assembly_t assembly{};
    for(unsigned n=0;n<24;n++) {
        assert(a.next(1201,true,queue,p) && p.op==0x40 && p.len<=74);
        assert(kbm_net_accept(&assembly,p.payload,p.len)==(n==23 ? 1 : 0));
        kbm_packet_t ack{}; ack.op=0xc0; ack.len=2;
        assert(a.acknowledge(ack,1201));
    }
    assert(!std::memcmp(assembly.frame.data,frame.data,1514));
    assert(a.next(1202,true,queue,p)); a.timedOut(); assert(!a.pending() && !a.ready());
    // UART0 failure cannot mutate UART1; UART1 continues its own frame queue.
    assert(b.ready() && b.next(1202,true,queue,p) && p.op==0x40);
    b.timedOut(); activate(b,queue,10000,false);
    a.begin(true,0xfffffff0u); assert(!a.next(1,false,queue,p) && !a.transitioning());
    activate(a,queue,0xfffffff0u);
    a.rebooted(2000); assert(a.takeReset() && !a.takeReset() && !a.ready());
    assert(!a.next(3499,true,queue,p)); assert(a.next(3500,true,queue,p) && p.op==0x41);
    p.op=0xc1; p.len=2; p.payload[0]=0; p.payload[1]=0;
    assert(a.acknowledge(p,3500) && a.takeReset());
    assert(a.next(4000,true,queue,p) && p.op==0x43 && p.payload[0]==1);
    std::puts("PASS independent native UART sessions, real ACK gates, cycle, 1514-byte CRC, bounded queue, reboot");
}
void append_byte(std::uint8_t byte, void* context) { static_cast<std::vector<std::uint8_t>*>(context)->push_back(byte); }
void test_uart() {
    mock::Uart driver; kbm::UartIo io(driver);
    kbm_packet_t packet{}; packet.op=0x10; packet.len=3;
    packet.payload[0]=2; packet.payload[1]=3; packet.payload[2]=27;
    std::vector<std::uint8_t> encoded; kbm_packet_write(&packet,append_byte,&encoded);
    std::size_t offset=0;
    while(offset<encoded.size()) {
        driver.slots=2; auto before=offset;
        io.send<32>(encoded.data(),encoded.size(),offset); assert(offset-before<=2);
    }
    assert(driver.tx==encoded);
    kbm_decoder_t decoder; kbm_decoder_init(&decoder); unsigned frames=0;
    // Force the production receive template through every possible read boundary.
    for(auto byte:driver.tx) {
        driver.rx.push_back(byte);
        io.receive<64>([&](std::uint8_t value) { if(kbm_decoder_feed(&decoder,value)==KBM_DECODE_FRAME) ++frames; });
    }
    assert(frames==1 && decoder.packet.op==0x10 && !std::memcmp(decoder.packet.payload,packet.payload,3));
    kbm_reboot_guard_t gate{};
    assert(!kbm_reboot_ready(&gate,0,true,true,true));
    assert(!kbm_reboot_ready(&gate,1,false,false,true));
    assert(!kbm_reboot_ready(&gate,2,false,true,false));
    assert(!kbm_reboot_ready(&gate,0xfffffff0u,false,true,true));
    assert(!kbm_reboot_ready(&gate,83,false,true,true));
    assert(kbm_reboot_ready(&gate,84,false,true,true));
    std::puts("PASS injected UART FIFO, progressive escaped byte reads, partial writes, reboot ACK drain/wraparound");
}
void test_router() {
    kbm::EthernetRouter<2> router;
    const std::uint8_t a[]={2,0,0,0,0,1}, b[]={2,0,0,0,0,2}, broadcast[]={255,255,255,255,255,255};
    assert(router.targets(a,3)==3);
    router.learn(0,a); router.learn(1,b);
    assert(router.targets(a,3)==1 && router.targets(b,3)==2);
    assert(router.targets(b,1)==0 && router.targets(broadcast,2)==2);
    router.forget(0); assert(router.targets(a,3)==3);
    router.clear(); assert(router.targets(b,3)==3);
    std::puts("PASS native Ethernet MAC learning, directed/flood traffic, unavailable-port isolation");
}
static std::uint16_t sum(const std::uint8_t* p, unsigned n) {
    std::uint32_t value=0;
    for(unsigned i=0;i+1<n;i+=2) value+=(p[i]<<8)|p[i+1];
    if(n&1) value+=static_cast<std::uint16_t>(p[n-1])<<8;
    while(value>>16) value=(value&65535)+(value>>16);
    return static_cast<std::uint16_t>(~value);
}
static void put(std::uint8_t* p,unsigned n) { p[0]=static_cast<std::uint8_t>(n>>8); p[1]=static_cast<std::uint8_t>(n); }
kbm_net_frame_t request(std::uint8_t kind,std::uint8_t mac,std::uint8_t host=0,bool renew=false) {
    kbm_net_frame_t frame{}; frame.length=342; auto f=frame.data;
    std::memset(f,255,6); f[6]=2; f[11]=mac;
    put(f+12,0x800); f[14]=0x45; put(f+16,328); f[22]=64; f[23]=17;
    if(renew) { f[26]=192; f[27]=168; f[28]=127; f[29]=host; f[30]=192; f[31]=168; f[32]=127; f[33]=1; }
    else std::memset(f+30,255,4);
    put(f+24,sum(f+14,20)); put(f+34,68); put(f+36,67); put(f+38,308);
    auto b=f+42; b[0]=1; b[1]=1; b[2]=6; b[4]=42; b[28]=2; b[33]=mac;
    if(renew) std::memcpy(b+12,f+26,4);
    const std::uint8_t options[]={99,130,83,99,53,1,kind,255}; std::memcpy(b+236,options,sizeof(options));
    if(host && !renew) { const std::uint8_t select[]={50,4,192,168,127,host,54,4,192,168,127,1,255}; std::memcpy(b+243,select,sizeof(select)); }
    return frame;
}
const std::uint8_t* option(const kbm_net_frame_t& frame,std::uint8_t code) {
    for(unsigned at=282;at<frame.length && frame.data[at]!=255;) {
        auto key=frame.data[at++]; if(!key) continue;
        if(at>=frame.length) break;
        auto n=frame.data[at++]; if(at+n>frame.length) break;
        if(key==code) return frame.data+at;
        at+=n;
    }
    return nullptr;
}
void test_dhcp() {
    kbm_dhcp_t server{}; const std::uint8_t mac[]={2,0,0,0,0,99}; kbm_dhcp_init(&server,mac);
    kbm_net_frame_t reply{};
    for(std::uint8_t port=0;port<3;port++) {
        const std::uint8_t host=port==2 ? 1 : static_cast<std::uint8_t>(100+port);
        auto discover=request(1,static_cast<std::uint8_t>(port+1));
        assert(kbm_dhcp_process(&server,port,1000,&discover,&reply));
        assert(reply.length && reply.data[61]==host && *option(reply,53)==2);
        auto select=request(3,static_cast<std::uint8_t>(port+1),host);
        assert(kbm_dhcp_process(&server,port,1001,&select,&reply) && *option(reply,53)==5);
        assert(option(reply,54)[3]==1 && option(reply,3)[3]==1 && !option(reply,6));
        assert(sum(reply.data+14,20)==0);
        if(port==2) assert(!std::memcmp(option(reply,51),"\xff\xff\xff\xff",4));
        auto renew=request(3,static_cast<std::uint8_t>(port+1),host,true);
        assert(kbm_dhcp_process(&server,port,2000,&renew,&reply) && *option(reply,53)==5);
        assert(reply.data[33]==host && reply.data[5]==port+1); // Unicast renewal intercepted before PC delivery.
    }
    auto wrong=request(3,1,199);
    assert(kbm_dhcp_process(&server,0,3000,&wrong,&reply) && *option(reply,53)==6);
    wrong.data[296]=2; // Other server identifier (.2), no response.
    assert(kbm_dhcp_process(&server,0,3000,&wrong,&reply) && !reply.length);
    auto corrupt=request(1,1); corrupt.data[24]^=1;
    assert(kbm_dhcp_process(&server,0,3000,&corrupt,&reply) && !reply.length);
    auto identified=request(1,3);
    const std::uint8_t cid[]={61,7,1,2,0,0,0,0,3,255};
    std::memcpy(identified.data+285,cid,sizeof(cid));
    assert(kbm_dhcp_process(&server,2,3000,&identified,&reply));
    assert(reply.length && !std::memcmp(option(reply,61),cid+2,7));
    auto normal=request(1,1); put(normal.data+36,22);
    assert(!kbm_dhcp_process(&server,0,3000,&normal,&reply)); // Other .1 traffic is not stolen.
    put(normal.data+12,0x806); assert(!kbm_dhcp_process(&server,0,3000,&normal,&reply));
    kbm_dhcp_forget(&server,0);
    for(const auto& l:server.leases) assert(!l.used || l.ingress!=0);
    for(unsigned n=0;n<342;n++) { auto truncated=request(1,1); truncated.length=static_cast<std::uint16_t>(n); kbm_dhcp_process(&server,0,0,&truncated,&reply); }
    auto huge=request(1,1); huge.length=65535; assert(!kbm_dhcp_process(&server,0,0,&huge,&reply));
    std::puts("PASS firmware DHCP .1 server/PC, .100/.101 slaves, infinite PC lease, renewal intercept, no ARP hijack, malformed input");
}
void test_disk() {
    kbm::DiskReader disk;
    uint8_t config[9]={1}; kbm_net_put32(config+1,42); kbm_net_put32(config+5,1024);
    std::array<uint8_t,512> expected{}, output{};
    for(unsigned i=0;i<512;i++) expected[i]=static_cast<uint8_t>(i*37);
    assert(!disk.ready(0) && disk.configure(config,9,0));
    assert(disk.takeAttention() && !disk.takeAttention());
    assert(disk.read(1,1024,0,output.data(),512)==-1);
    assert(disk.read(1,10,0,output.data(),512)==0);
    kbm_packet_t request{}; assert(disk.request(1,request) && request.op==0xf8 && request.len==14 && kbm_net_u16(request.payload+12)==1);
    assert(disk.request(2,request)); // Queue full: not published, request still available.
    disk.published(2); assert(!disk.request(501,request) && disk.request(502,request));
    kbm_packet_t data{}; data.op=0x45; data.len=82;
    memcpy(data.payload,request.payload,12);
    kbm_net_put32(data.payload+14,kbm_net_crc(expected.data(),512));
    auto stale=data; stale.payload[0]^=1; assert(!disk.accept(stale.payload,stale.len));
    for(uint16_t offset=0;offset<512;offset+=64) {
        kbm_net_put16(data.payload+12,offset); memcpy(data.payload+18,expected.data()+offset,64);
        assert(disk.accept(data.payload,data.len)); assert(disk.accept(data.payload,data.len)); // Exact duplicate.
        if(offset<448) assert(disk.read(10,10,0,output.data(),512)==0);
    }
    assert(disk.read(20,10,0,output.data(),512)==512 && output==expected);
    assert(disk.read(20,10,500,output.data(),512)==12);
    assert(disk.configure(config,9,1000) && !disk.takeAttention()); // Heartbeat preserves cache.
    assert(disk.read(1001,10,0,output.data(),512)==512);
    assert(!disk.ready(6000)); disk.eject(); assert(!disk.ready(1002));
    kbm_net_put32(config+1,43); assert(disk.configure(config,9,2000) && disk.ready(2000));
    assert(disk.read(2000,10,0,output.data(),512)==0); assert(disk.request(2000,request));
    assert(!disk.fail(stale.payload,12)); assert(disk.fail(request.payload,12));
    assert(disk.read(2001,10,0,output.data(),512)==-1);
    assert(disk.read(2002,10,0,output.data(),512)==0); assert(disk.request(2002,request));
    memcpy(data.payload,request.payload,12); kbm_net_put32(data.payload+14,0);
    for(uint16_t offset=0;offset<512;offset+=64) {
        kbm_net_put16(data.payload+12,offset); memcpy(data.payload+18,expected.data()+offset,64);
        assert(disk.accept(data.payload,data.len)==(offset<448));
    }
    assert(disk.read(2100,10,0,output.data(),512)==-1);
    assert(disk.read(2101,10,0,output.data(),512)==0);
    assert(disk.configure(config,9,5000)); // Refresh lease, not the 4s read timeout.
    assert(disk.read(6101,10,0,output.data(),512)==-1);
    config[0]=0; kbm_net_put32(config+5,0); assert(disk.configure(config,9,6102) && !disk.ready(6102));
    std::puts("PASS virtual disk nonblocking reads, CRC, generations, duplicate fragments, timeout, heartbeat, eject, OFF");
}
void test_disk_cache96() {
    kbm::DiskReader disk;
    uint8_t config[9]={1};kbm_net_put32(config+1,77);kbm_net_put32(config+5,100);
    assert(disk.configure(config,9,0));
    std::array<uint8_t,4096> out{};
    auto fill=[&](uint32_t lba,uint32_t bytes) {
        assert(disk.read(1,lba,0,out.data(),bytes)==0);
        kbm_packet_t request{};assert(disk.request(1,request));
        const uint16_t count=kbm_net_u16(request.payload+12);
        assert(count==(bytes+511)/512 && count<=8);
        kbm_disk_transfer_t transfer{};transfer.len=18+count*512;
        memcpy(transfer.payload,request.payload,12);kbm_net_put16(transfer.payload+12,0);
        for(unsigned i=0;i<count*512;i++) transfer.payload[18+i]=static_cast<uint8_t>((lba+i/512)*13+i%512);
        kbm_net_put32(transfer.payload+14,kbm_net_crc(transfer.payload+18,count*512));
        assert(disk.accept(transfer.payload,transfer.len));assert(disk.accept(transfer.payload,transfer.len));
    };
    for(uint32_t base=0;base<96;base+=8) fill(base,4096);
    for(uint32_t lba=0;lba<96;lba++) {assert(disk.read(2,lba,0,out.data(),512)==512);assert(out[0]==uint8_t(lba*13));}
    assert(disk.read(2,0,0,out.data(),512)==512); // Touch0, then evict least-recently-used1.
    fill(96,512);
    assert(disk.read(2,0,0,out.data(),512)==512);
    assert(disk.read(2,1,0,out.data(),512)==0);
    kbm_packet_t pending{};assert(disk.request(2,pending)&&kbm_net_u16(pending.payload+12)==1);
    // Failed CRC must never publish any sector in a new multi-sector block.
    assert(disk.read(3,97,0,out.data(),1536)==0);assert(disk.request(3,pending));
    assert(kbm_net_u16(pending.payload+12)==3);
    kbm_disk_transfer_t bad{};bad.len=18+1536;memcpy(bad.payload,pending.payload,12);
    kbm_net_put32(bad.payload+14,123);assert(!disk.accept(bad.payload,bad.len));
    assert(disk.read(4,97,0,out.data(),512)==-1);
    assert(disk.read(4,98,0,out.data(),512)==0); // No partial cache commit after bad CRC.
    kbm_net_put32(config+1,78);assert(disk.configure(config,9,5));
    assert(disk.read(5,0,0,out.data(),512)==0); // Refresh invalidates every sector.
    std::puts("PASS48KiB96-sector LRU, variable1..8-sector reads, partial1.5KiB block, CRC atomicity, refresh invalidation");
}
int main() { test_mode(); test_optional_functions(); test_links(); test_uart(); test_router(); test_dhcp(); test_disk(); test_disk_cache96(); std::puts("All firmware mock tests passed (no hardware/USB/HID access)."); }
