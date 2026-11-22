#ifndef KBM_SLAVE_SERIAL_BRIDGE_H
#define KBM_SLAVE_SERIAL_BRIDGE_H
// Called only by core 1, the TinyUSB owner.
void serial_bridge_task(void);
#endif
