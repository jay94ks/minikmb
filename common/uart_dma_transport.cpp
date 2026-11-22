#include "uart_dma_transport.h"
#include "firmware/dma_rx_cursor.hpp"
#include "hardware/dma.h"
#include "hardware/irq.h"
#include "pico/stdlib.h"
#include <cassert>

#ifndef KBM_DMA_UART_COUNT
#define KBM_DMA_UART_COUNT 2
#endif
namespace { constexpr unsigned Half=1024, Capacity=2*Half; }
struct kbm_uart_dma {
    alignas(2048) volatile uint16_t words[Capacity]{};
    uart_inst_t *uart{};
    uint rx[2]{},tx{};
    uint32_t completed{},sequence{},discontinuities{},seen_discontinuities{};
    kbm::DmaRxCursor<Capacity> cursor;
    uint32_t produced() const {
        for(;;) {
            const auto before=__atomic_load_n(&sequence,__ATOMIC_ACQUIRE);
            if(before&1u) continue;
            const auto done=__atomic_load_n(&completed,__ATOMIC_RELAXED);
            const auto remaining=dma_channel_hw_addr(rx[(done/Half)&1u])->transfer_count;
            if(before==__atomic_load_n(&sequence,__ATOMIC_ACQUIRE)) return done+Half-remaining;
        }
    }
    uint16_t word(size_t index) const { return words[index]; }
};
namespace {
kbm_uart_dma contexts[KBM_DMA_UART_COUNT];
unsigned count;
void rx_complete() {
    const auto limit=__atomic_load_n(&count,__ATOMIC_ACQUIRE);
    for(unsigned i=0;i<limit;i++) {
        auto& d=contexts[i];
        const uint32_t pending=dma_hw->ints0;
        if((pending&(1u<<d.rx[0]))&&(pending&(1u<<d.rx[1]))) {
            __atomic_store_n(&d.discontinuities,__atomic_load_n(&d.discontinuities,__ATOMIC_RELAXED)+1u,__ATOMIC_RELEASE);
        }
        // If both halves completed before IRQ entry, process in wire order.
        for(unsigned budget=0;budget<2;budget++) {
            const auto done=__atomic_load_n(&d.completed,__ATOMIC_RELAXED);
            const auto half=(done/Half)&1u;
            const auto channel=d.rx[half];
            if(!dma_irqn_get_channel_status(0,channel)) break;
            const auto sequence=__atomic_load_n(&d.sequence,__ATOMIC_RELAXED);
            __atomic_store_n(&d.sequence,sequence+1u,__ATOMIC_RELEASE);
            dma_irqn_acknowledge_channel(0,channel);
            __atomic_store_n(&d.completed,done+Half,__ATOMIC_RELAXED);
            __atomic_store_n(&d.sequence,sequence+2u,__ATOMIC_RELEASE);
        }
    }
}
}
extern "C" kbm_uart_dma_t *kbm_uart_dma_init(uart_inst_t *uart) {
    const auto index=__atomic_load_n(&count,__ATOMIC_RELAXED);
    assert(index<KBM_DMA_UART_COUNT);
    auto& d=contexts[index];d.uart=uart;
    d.rx[0]=dma_claim_unused_channel(true);d.rx[1]=dma_claim_unused_channel(true);d.tx=dma_claim_unused_channel(true);
    for(unsigned half=0;half<2;half++) {
        const auto channel=d.rx[half];
        auto config=dma_channel_get_default_config(channel);
        channel_config_set_transfer_data_size(&config,DMA_SIZE_16); // Keep UART DR error flags.
        channel_config_set_read_increment(&config,false);channel_config_set_write_increment(&config,true);
        // Each aligned2KiB half wraps itself. Chaining reloads TRANS_COUNT in
        // hardware, so continuous capture never depends on IRQ rearming.
        channel_config_set_ring(&config,true,11);
        channel_config_set_dreq(&config,uart_get_dreq(uart,false));
        channel_config_set_chain_to(&config,d.rx[half^1u]);
        dma_channel_configure(channel,&config,const_cast<uint16_t*>(d.words+half*Half),&uart_get_hw(uart)->dr,Half,false);
        dma_channel_set_irq0_enabled(channel,true);
    }
    auto tx=dma_channel_get_default_config(d.tx);
    channel_config_set_transfer_data_size(&tx,DMA_SIZE_8);
    channel_config_set_read_increment(&tx,true);channel_config_set_write_increment(&tx,false);
    channel_config_set_dreq(&tx,uart_get_dreq(uart,true));
    dma_channel_configure(d.tx,&tx,&uart_get_hw(uart)->dr,nullptr,0,false);
    // DMAONERR=0 keeps RX requests running when UART error flags occur; capture
    // those flags in16-bit words and invalidate the decoder in foreground.
    hw_clear_bits(&uart_get_hw(uart)->dmacr,UART_UARTDMACR_DMAONERR_BITS);
    hw_set_bits(&uart_get_hw(uart)->dmacr,UART_UARTDMACR_RXDMAE_BITS|UART_UARTDMACR_TXDMAE_BITS);
    __atomic_store_n(&count,index+1u,__ATOMIC_RELEASE);
    if(index==0) { irq_set_exclusive_handler(DMA_IRQ_0,rx_complete);irq_set_priority(DMA_IRQ_0,PICO_HIGHEST_IRQ_PRIORITY);irq_set_enabled(DMA_IRQ_0,true); }
    dma_start_channel_mask(1u<<d.rx[0]);return &d;
}
extern "C" int kbm_uart_dma_read(kbm_uart_dma_t *d,uint8_t *byte) {
    const auto discontinuities=__atomic_load_n(&d->discontinuities,__ATOMIC_ACQUIRE);
    if(discontinuities!=d->seen_discontinuities) {
        d->seen_discontinuities=discontinuities;d->cursor.invalidate(*d);return -1;
    }
    const int result=d->cursor.read(*d,*byte);
    if(result<0) uart_get_hw(d->uart)->rsr=0;
    return result;
}
extern "C" uint32_t kbm_uart_dma_errors(kbm_uart_dma_t *d) { return d->cursor.errors(); }
extern "C" void kbm_uart_dma_discard(kbm_uart_dma_t *d) { d->cursor.discard(*d);uart_get_hw(d->uart)->rsr=0; }
extern "C" void kbm_uart_dma_tx_start(kbm_uart_dma_t *d,const uint8_t *bytes,size_t length) {
    assert(!dma_channel_is_busy(d->tx));
    dma_channel_set_read_addr(d->tx,bytes,false);dma_channel_set_trans_count(d->tx,length,true);
}
extern "C" bool kbm_uart_dma_tx_busy(kbm_uart_dma_t *d) { return dma_channel_is_busy(d->tx); }
