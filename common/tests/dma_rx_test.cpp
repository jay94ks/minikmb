#include "firmware/dma_rx_cursor.hpp"
#include "protocol.h"
#include <array>
#include <cassert>
#include <cstdio>

struct MockDma {
    std::array<uint16_t,2048> words{};
    uint32_t head{};
    bool overtake{};
    uint32_t produced() const { return head; }
    uint16_t word(size_t index) { const auto value=words[index];if(overtake){head+=2049;overtake=false;}return value; }
    void write(uint16_t value) { words[head++&2047]=value; }
};
int main() {
    MockDma dma;
    kbm::DmaRxCursor<2048> cursor;
    uint8_t byte;
    assert(cursor.read(dma,byte)==0);
    // Prefix is visible immediately, never wait for a1024-word DMA half.
    dma.write(42);assert(cursor.read(dma,byte)==1&&byte==42);
    for(unsigned n=0;n<2048;n++)dma.write(n&255);
    for(unsigned n=0;n<2048;n++){assert(cursor.read(dma,byte)==1);assert(byte==static_cast<uint8_t>(n));}
    for(unsigned n=0;n<2049;n++)dma.write(7);
    assert(cursor.read(dma,byte)==-1);assert(cursor.errors()==1);assert(cursor.read(dma,byte)==0);
    dma.write(0x800);assert(cursor.read(dma,byte)==-1);assert(cursor.errors()==2); // UART OE flag retained.
    dma.write(5);dma.overtake=true;assert(cursor.read(dma,byte)==-1);assert(cursor.errors()==3);
    dma.head=UINT32_MAX-50;cursor.discard(dma);
    for(unsigned n=0;n<200;n++){dma.write(n&255);assert(cursor.read(dma,byte)==1);assert(byte==n);}
    cursor.invalidate(dma);assert(cursor.errors()==4);
    static std::array<uint8_t,4114> payload;
    for(unsigned n=0;n<payload.size();n++)payload[n]=(n%3==0)?2:(n%3==1)?3:27;
    struct Wire { std::array<uint8_t,8242> bytes{};size_t size{}; } wire;
    kbm_frame_write(0x45,payload.size(),payload.data(),[](uint8_t b,void *p){auto& w=*static_cast<Wire*>(p);w.bytes[w.size++]=b;},&wire);
    kbm_decoder_t decoder;
    static std::array<uint8_t,4118> decoded;
    kbm_decoder_init_large(&decoder,decoded.data(),decoded.size());
    unsigned frames=0;
    for(size_t at=0;at<wire.size;) {
        for(unsigned budget=0;budget<64&&at<wire.size;budget++)dma.write(wire.bytes[at++]);
        int result;
        while((result=cursor.read(dma,byte))!=0) {
            assert(result==1);
            const auto state=kbm_decoder_feed(&decoder,byte);
            assert(state==KBM_DECODE_MORE||state==KBM_DECODE_LARGE_FRAME);
            frames+=state==KBM_DECODE_LARGE_FRAME;
        }
    }
    assert(frames==1&&decoder.packet.len==payload.size());
    for(unsigned n=0;n<payload.size();n++)assert(decoded[n]==payload[n]);
    std::puts("PASS DMA RX injected backend: partial prefix, FIFO, overwrite/UART flags,32-bit wrap, raced read,4KiB escaped progressive decode");
}
