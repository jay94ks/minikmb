#pragma once
#include <cstddef>
#include <cstdint>

namespace kbm {
// Hardware owns words/progress; this cursor never waits for a DMA block to fill.
// Backend injection keeps partial-block, overwrite and UART-error tests portable.
template<std::size_t Capacity>
class DmaRxCursor {
    static_assert(Capacity && (Capacity&(Capacity-1))==0);
public:
    template<class Backend> int read(Backend& backend,std::uint8_t& byte) {
        const auto head=backend.produced();
        if(head-consumed_>Capacity) return lost(head);
        if(head==consumed_) return 0;
        const auto word=backend.word(consumed_&(Capacity-1));
        const auto after=backend.produced();
        if(after-consumed_>Capacity) return lost(after); // DMA overtook this read.
        ++consumed_;
        if(word&0xf00u) return lost(after);
        byte=static_cast<std::uint8_t>(word);return 1;
    }
    template<class Backend> void discard(Backend& backend) { consumed_=backend.produced(); }
    template<class Backend> void invalidate(Backend& backend) { (void)lost(backend.produced()); }
    std::uint32_t errors() const { return errors_; }
private:
    int lost(std::uint32_t head) { consumed_=head;++errors_;return -1; }
    std::uint32_t consumed_{},errors_{};
};
}
