#pragma once
#include "disk_protocol.h"
#include <array>
#include <cstring>
#include <algorithm>
namespace kbm {
// Core-1-owned sector-granular LRU:96 x512=48KiB data, no heap.
// A separate4KiB staging buffer is never visible until full-block CRC passes.
class DiskReader {
public:
    bool configure(const uint8_t* p,uint8_t n,uint32_t now) {
        if(!kbm_disk_config_valid(p,n)) return false;
        const bool changed=enabled_!=(p[0]!=0)||generation_!=kbm_net_u32(p+1)||sectors_!=kbm_net_u32(p+5);
        if(changed) {
            pending_=completed_=failed_=ejected_=false;attention_=true;received_=0;
            for(auto& slot:cache_) slot.valid=false;
            enabled_=p[0]!=0;generation_=kbm_net_u32(p+1);sectors_=kbm_net_u32(p+5);
        }
        contact_=now;return true;
    }
    bool ready(uint32_t now) const { return enabled_&&!ejected_&&uint32_t(now-contact_)<5000u; }
    bool takeAttention() { bool result=attention_;attention_=false;return result; }
    void eject() { ejected_=true;pending_=false;for(auto& slot:cache_) slot.valid=false; }
    uint32_t sectors() const { return sectors_; }
    bool enabled() const { return enabled_; }
    int32_t read(uint32_t now,uint32_t lba,uint32_t offset,void* output,uint32_t count) {
        if(!ready(now)||lba>=sectors_||offset>=512||!count) return -1;
        count=std::min<uint32_t>(count,4096u);
        count=static_cast<uint32_t>(std::min<uint64_t>(count,uint64_t(sectors_-lba)*512-offset));
        uint32_t copied=0,current=lba,in_sector=offset;
        while(copied<count) {
            Slot* slot=find(current);
            if(!slot) break;
            const auto n=std::min(count-copied,512u-in_sector);
            std::memcpy(static_cast<uint8_t*>(output)+copied,slot->data.data()+in_sector,n);
            slot->stamp=++stamp_;copied+=n;in_sector=0;++current;
        }
        if(copied) return static_cast<int32_t>(copied); // A cached prefix may be returned immediately.
        if(pending_&&lba==lba_&&(failed_||uint32_t(now-started_)>=4000u)) { pending_=false;return -1; }
        if(!pending_||lba!=lba_) {
            pending_=true;completed_=failed_=false;received_=0;lba_=lba;
            // Exactly enough sectors for the requested bytes; maximum8, no fixed4KiB padding.
            count_=static_cast<uint16_t>(std::min<uint32_t>(8,(offset+count+511)/512));
            ++request_;if(!request_) ++request_;started_=now;sent_=false;
        }
        return 0;
    }
    template<class Packet> bool request(uint32_t now,Packet& p) {
        if(!pending_||completed_||failed_||!ready(now)||(sent_&&uint32_t(now-sent_at_)<500u)) return false;
        p={};p.op=KBM_DISK_READ_EVENT;p.len=14;
        kbm_net_put32(p.payload,generation_);kbm_net_put32(p.payload+4,request_);kbm_net_put32(p.payload+8,lba_);
        kbm_net_put16(p.payload+12,count_);return true;
    }
    void published(uint32_t now) { sent_=true;sent_at_=now; }
    bool fail(const uint8_t* p,uint8_t n) { if(n!=12||!matches(p)) return false;failed_=true;return true; }
    bool accept(const uint8_t* p,uint16_t n) {
        if(!kbm_disk_data_valid(p,n)||!matches(p)||failed_) return false;
        const auto off=kbm_net_u16(p+12),bytes=static_cast<uint16_t>(n-KBM_DISK_HEADER);
        const auto crc=kbm_net_u32(p+14);
        if(uint32_t(off)+bytes>uint32_t(count_)*512) return false;
        if(off<received_) return crc==crc_&&off+bytes<=received_&&!std::memcmp(staging_.data()+off,p+KBM_DISK_HEADER,bytes);
        if(off!=received_||(off&&crc!=crc_)) return false;
        if(!off) crc_=crc;
        std::memcpy(staging_.data()+off,p+KBM_DISK_HEADER,bytes);received_+=bytes;
        if(received_==count_*512) {
            if(kbm_net_crc(staging_.data(),received_)!=crc_) { failed_=true;return false; }
            for(uint16_t i=0;i<count_;i++) {
                Slot* slot=find(lba_+i);
                if(!slot) {
                    slot=&cache_[0];
                    for(auto& candidate:cache_) {
                        if(!candidate.valid) { slot=&candidate;break; }
                        if(uint32_t(stamp_-candidate.stamp)>uint32_t(stamp_-slot->stamp)) slot=&candidate;
                    }
                }
                slot->valid=false;slot->lba=lba_+i;
                std::memcpy(slot->data.data(),staging_.data()+i*512,512);
                slot->stamp=++stamp_;slot->valid=true;
            }
            completed_=true;
        }
        return true;
    }
private:
    struct Slot { std::array<uint8_t,512> data{};uint32_t lba{},stamp{};bool valid{}; };
    Slot* find(uint32_t lba) { for(auto& slot:cache_) if(slot.valid&&slot.lba==lba) return &slot;return nullptr; }
    bool matches(const uint8_t* p) const { return pending_&&kbm_net_u32(p)==generation_&&kbm_net_u32(p+4)==request_&&kbm_net_u32(p+8)==lba_; }
    std::array<Slot,KBM_DISK_CACHE_SECTORS> cache_{};
    std::array<uint8_t,4096> staging_{};
    uint32_t generation_{},sectors_{},request_{},lba_{},crc_{},contact_{},started_{},sent_at_{},stamp_{};
    uint16_t received_{},count_{};
    bool enabled_{},attention_{},pending_{},completed_{},failed_{},sent_{},ejected_{};
};
static_assert(sizeof(DiskReader)<56*1024);
}
