#pragma once
#include <stdbool.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// UART core requests/polls; USB core owns every TinyUSB call.
void kbm_wake_request(void);
bool kbm_wake_poll(uint8_t *result);
void kbm_wake_cancel(void);
void kbm_wake_task(void);
void kbm_wake_suspend(bool allowed);
void kbm_wake_resume(void);
void kbm_wake_automatic(void);
#ifdef __cplusplus
}
#endif
