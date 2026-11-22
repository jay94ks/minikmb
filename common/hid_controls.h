#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#define KBM_SYSTEM_DOWN 0x12u
#define KBM_SYSTEM_UP 0x13u
#define KBM_SYSTEM_POWER_DOWN 0x81u // Generic Desktop page01, not keyboard page07.
#define KBM_SYSTEM_REPORT_BYTES 5u
static inline int kbm_hid_system_index(uint8_t usage) {
    static const uint8_t usages[]={
#define KBM_SYSTEM_USAGE(value) value,
#include "system_controls.def"
#undef KBM_SYSTEM_USAGE
    };
    for(unsigned i=0;i<sizeof(usages);i++) if(usages[i]==usage) return (int)i;
    return -1;
}
static inline bool kbm_hid_system_set(uint8_t state[KBM_SYSTEM_REPORT_BYTES],uint8_t usage,bool down) {
    const int index=kbm_hid_system_index(usage);
    if(index<0) return false;
    const uint8_t mask=(uint8_t)(1u<<((unsigned)index&7u));
    if(down) state[(unsigned)index>>3u]|=mask;
    else state[(unsigned)index>>3u]&=(uint8_t)~mask;
    return true;
}
static inline bool kbm_hid_op_can_wake(uint8_t op) {
    return op==0x10u || op==0x20u || op==0x21u; // New keyboard/mouse activity only.
}
static inline bool kbm_hid_resume_needed(bool report_activity,uint32_t generation,uint32_t suspend_generation) {
    return report_activity || generation!=suspend_generation;
}
static inline bool kbm_hid_power_current(uint32_t command_epoch,uint32_t usb_epoch,bool ready) {
    return ready && command_epoch==usb_epoch;
}
static inline void kbm_hid_key_set(uint8_t keys[32],uint8_t key,bool down) {
    const uint8_t mask=(uint8_t)(1u<<(key&7u));
    if(down) keys[key>>3u]|=mask;else keys[key>>3u]&=(uint8_t)~mask;
}
static inline bool kbm_hid_key_get(const uint8_t keys[32],uint8_t key) {
    return (keys[key>>3u]&(uint8_t)(1u<<(key&7u)))!=0;
}
static inline void kbm_hid_keyboard_report(const uint8_t keys[32],uint8_t report[8]) {
    memset(report,0,8);
    for(unsigned key=0xe0;key<=0xe7;key++)
        if(kbm_hid_key_get(keys,(uint8_t)key)) report[0]|=(uint8_t)(1u<<(key-0xe0));
    unsigned count=0;
    for(unsigned key=1;key<=255;key++) {
        if(key>=0xe0&&key<=0xe7) continue;
        if(!kbm_hid_key_get(keys,(uint8_t)key)) continue;
        if(count<6) report[2+count]=(uint8_t)key;
        ++count;
    }
    if(count>6) memset(report+2,1,6); // Full state remains intact through6KRO.
}
