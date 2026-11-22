#ifndef MINIKMB_TUSB_CONFIG_H
#define MINIKMB_TUSB_CONFIG_H

#include "target_features.h"

#define CFG_TUSB_MCU OPT_MCU_STM32H7
#define CFG_TUSB_OS OPT_OS_FREERTOS
#ifdef MINIKMB_TUSB_TARGET_INSTANCE
#define CFG_TUSB_RHPORT1_MODE (OPT_MODE_DEVICE | OPT_MODE_HIGH_SPEED)
#define CFG_TUD_CDC_RX_BUFSIZE 4096
#define CFG_TUD_CDC_TX_BUFSIZE 4096
#if MINIKMB_TARGET_MSC
#define CFG_TUD_MSC 1
#define CFG_TUD_MSC_EP_BUFSIZE 8192
#endif
#if MINIKMB_TARGET_NCM
#define CFG_TUD_NCM 1
#define CFG_TUD_NCM_IN_NTB_MAX_SIZE 6144
#define CFG_TUD_NCM_OUT_NTB_MAX_SIZE 6144
#define CFG_TUD_NCM_IN_NTB_N 2
#define CFG_TUD_NCM_OUT_NTB_N 2
#define CFG_TUD_NCM_DEFAULT_LINK_UP 0
#define CFG_TUD_NCM_LINK_BPS 1000000U
#define CFG_TUD_NCM_MEM_SECTION __attribute__((section(".ncm_dma")))
#endif
#if defined(MINIKMB_TARGET_HID) && MINIKMB_TARGET_HID
#define CFG_TUD_HID 2
#define CFG_TUD_HID_EP_BUFSIZE 8
#endif
#define CFG_TUD_DWC2_SLAVE_ENABLE 1
#define CFG_TUD_DWC2_DMA_ENABLE 0
#define CFG_TUD_MEM_SECTION __attribute__((section(".usb_hs_dma")))
#else
#define CFG_TUSB_RHPORT0_MODE (OPT_MODE_DEVICE | OPT_MODE_FULL_SPEED)
#define CFG_TUD_MSC 1
#define CFG_TUD_MSC_EP_BUFSIZE 8192
#if MINIKMB_MANAGEMENT_FAST_CDC
#define CFG_TUD_CDC_RX_BUFSIZE 4096
#define CFG_TUD_CDC_RX_EPSIZE 2048
#define CFG_TUD_CDC_RX_NEED_ZLP 1
#else
#define CFG_TUD_CDC_RX_BUFSIZE 512
#endif
#if MINIKMB_MANAGEMENT_FAST_CDC
/* A complete management frame fits in one static IN endpoint staging buffer. */
#define CFG_TUD_CDC_TX_BUFSIZE 4096
#define CFG_TUD_CDC_TX_EPSIZE 2048
#else
#define CFG_TUD_CDC_TX_BUFSIZE 512
#define CFG_TUD_CDC_TX_EPSIZE 256
#endif
#define CFG_TUD_DWC2_SLAVE_ENABLE 1
#define CFG_TUD_DWC2_DMA_ENABLE 0
#define CFG_TUD_MEM_SECTION __attribute__((section(".usb_fs_dma")))
#endif

#define CFG_TUD_ENDPOINT0_SIZE 64
#define CFG_TUD_CDC 1
#define CFG_TUD_CDC_NOTIFY 1
#define CFG_TUD_CDC_TX_OVERWRITABLE_IF_NOT_CONNECTED 0
#define CFG_TUD_MEM_ALIGN __attribute__((aligned(32)))

#endif
