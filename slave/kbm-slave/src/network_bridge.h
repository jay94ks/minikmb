#ifndef KBM_NETWORK_BRIDGE_H
#define KBM_NETWORK_BRIDGE_H
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif
void network_bridge_init(void);
void network_bridge_task(void);
// Core 1: false during deliberate USB re-enumeration, including a hidden NCM.
bool network_bridge_usb_available(void);
#ifdef __cplusplus
}
#endif
#endif
