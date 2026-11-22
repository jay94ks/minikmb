#pragma once
#include "../boot_protocol.h"
#include <array>
#include <cstdint>
#include <cstring>
namespace kbm {
class BootIdentity {
public:
    // -1 invalid,0 duplicate retry,1 new boot. A retry never drains new work.
    int observe(const uint8_t *p,uint16_t n) {
        if(!kbm_boot_valid(p,n)) return -1;
        if(seen_ && std::memcmp(token_.data(),p+KBM_BOOT_TOKEN_OFFSET,token_.size())==0) return 0;
        std::memcpy(token_.data(),p+KBM_BOOT_TOKEN_OFFSET,token_.size());seen_=true;return 1;
    }
    void acknowledgement(uint8_t *p) const { p[0]=KBM_BOOT_SCHEMA;std::memcpy(p+1,token_.data(),token_.size()); }
private:
    std::array<uint8_t,KBM_BOOT_TOKEN_BYTES> token_{};
    bool seen_{};
};
// Snapshot old queue boundaries; retire at most one request per tick. USB queue
// pressure must never block servicing the other UART or lose a cancellation.
class BootDrain {
public:
    void begin(std::array<uint16_t,3> counts,bool active,uint16_t sequence) {
        counts_=counts;pending_=active;sequence_=sequence;
    }
    template<class Pop,class Emit> bool tick(Pop pop,Emit emit) {
        if(pending_) { if(!emit(sequence_)) return false;pending_=false;return done(); }
        for(unsigned i=0;i<counts_.size();i++) if(counts_[i]) {
            if(!pop(i,sequence_)) return false;
            --counts_[i];pending_=true;return false;
        }
        return true;
    }
    bool done() const { return !pending_ && counts_[0]==0 && counts_[1]==0 && counts_[2]==0; }
private:
    std::array<uint16_t,3> counts_{};
    bool pending_{};
    uint16_t sequence_{};
};
}
