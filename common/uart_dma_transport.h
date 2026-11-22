#ifndef KBM_UART_DMA_TRANSPORT_H
#define KBM_UART_DMA_TRANSPORT_H
#include "hardware/uart.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct kbm_uart_dma kbm_uart_dma_t;
// Returns statically owned context. Two chained RX channels + one TX channel.
kbm_uart_dma_t *kbm_uart_dma_init(uart_inst_t *uart);
// 1=byte,0=none,-1=overrun/UART error; caller must reset progressive decoder.
int kbm_uart_dma_read(kbm_uart_dma_t *dma,uint8_t *byte);
uint32_t kbm_uart_dma_errors(kbm_uart_dma_t *dma);
void kbm_uart_dma_discard(kbm_uart_dma_t *dma);
void kbm_uart_dma_tx_start(kbm_uart_dma_t *dma,const uint8_t *bytes,size_t length);
bool kbm_uart_dma_tx_busy(kbm_uart_dma_t *dma);
#ifdef __cplusplus
}
#endif
#endif
