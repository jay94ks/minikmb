#ifndef KBM_SLAVE_TUSB_CONFIG_H
#define KBM_SLAVE_TUSB_CONFIG_H

#ifdef __cplusplus
extern "C" {
#endif

#ifndef BOARD_TUD_RHPORT
#define BOARD_TUD_RHPORT 0
#endif

#ifndef BOARD_TUD_MAX_SPEED
#define BOARD_TUD_MAX_SPEED OPT_MODE_DEFAULT_SPEED
#endif

#ifndef CFG_TUSB_MCU
#error CFG_TUSB_MCU must be defined by the Pico SDK
#endif

#ifndef CFG_TUSB_OS
#define CFG_TUSB_OS OPT_OS_NONE
#endif
#define CFG_TUSB_DEBUG 0
#define CFG_TUD_ENABLED 1
#define CFG_TUD_MAX_SPEED BOARD_TUD_MAX_SPEED

#ifndef CFG_TUSB_MEM_SECTION
#define CFG_TUSB_MEM_SECTION
#endif
#ifndef CFG_TUSB_MEM_ALIGN
#define CFG_TUSB_MEM_ALIGN __attribute__((aligned(4)))
#endif

#define CFG_TUD_ENDPOINT0_SIZE 64
#define CFG_TUD_HID 1
#define CFG_TUD_NCM 1
#define CFG_TUD_NET_MTU 1514
#define CFG_TUD_NCM_IN_NTB_MAX_SIZE 2048
#define CFG_TUD_NCM_OUT_NTB_MAX_SIZE 2048
#define CFG_TUD_NCM_IN_NTB_N 2
#define CFG_TUD_NCM_OUT_NTB_N 2
#define CFG_TUD_CDC 1
#define CFG_TUD_CDC_RX_BUFSIZE 512
#define CFG_TUD_CDC_TX_BUFSIZE 512
#define CFG_TUD_CDC_EP_BUFSIZE 64
#define CFG_TUD_MSC 1
#define CFG_TUD_MSC_EP_BUFSIZE 4096
#define CFG_TUD_MIDI 0
#define CFG_TUD_VENDOR 0
#define CFG_TUD_HID_EP_BUFSIZE 16

#ifdef __cplusplus
}
#endif

#endif

