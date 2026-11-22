#ifndef KBM_DISK_BRIDGE_H
#define KBM_DISK_BRIDGE_H
#include <stdint.h>
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif
void disk_bridge_task(void);
bool disk_bridge_visible(void);
#ifdef __cplusplus
}
#endif
#endif
