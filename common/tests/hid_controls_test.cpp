#include "hid_controls.h"
#include "usb_session.h"
#include "hid_diagnostics.h"
#include <array>
#include <cassert>
#include <cstdio>
int main() {
    uint8_t diagnostic[KBM_HID_DIAGNOSTICS_BYTES]={KBM_HID_DIAGNOSTICS_SCHEMA};
    assert(kbm_hid_diagnostics_valid(diagnostic,sizeof(diagnostic)));
    diagnostic[1]=128;assert(!kbm_hid_diagnostics_valid(diagnostic,sizeof(diagnostic)));
    diagnostic[1]=0;diagnostic[2]=33;assert(!kbm_hid_diagnostics_valid(diagnostic,sizeof(diagnostic)));
    diagnostic[2]=32;diagnostic[3]=4;assert(!kbm_hid_diagnostics_valid(diagnostic,sizeof(diagnostic)));
    diagnostic[3]=3;assert(!kbm_hid_diagnostics_valid(diagnostic,sizeof(diagnostic)-1));
    unsigned valid=0;
    for(unsigned usage=0;usage<256;usage++) {
        uint8_t system[KBM_SYSTEM_REPORT_BYTES]={0};
        if(kbm_hid_system_index(usage)>=0) {
            const unsigned index=valid++;
            assert((unsigned)kbm_hid_system_index(usage)==index);
            assert(kbm_hid_system_set(system,usage,true));
            for(unsigned byte=0;byte<sizeof(system);byte++) assert(system[byte]==(byte==index/8u?(1u<<(index%8u)):0));
            assert(kbm_hid_system_set(system,usage,false));
            for(auto value:system) assert(value==0);
        } else assert(!kbm_hid_system_set(system,usage,true));
    }
    assert(valid==40 && kbm_hid_system_index(0x99)<0); // Indicator is not an input.
    kbm_usb_session_t session{};
    assert(kbm_usb_session_poll(&session,false,false,0)==0);
    assert(kbm_usb_session_poll(&session,true,false,0)==KBM_USB_SESSION_MOUNT);
    assert(kbm_usb_session_poll(&session,true,true,0)==KBM_USB_SESSION_SUSPEND);
    // Host bus reset can occur with NO unmount callback and same sampled state.
    assert(kbm_usb_session_poll(&session,true,true,1)==KBM_USB_SESSION_RESET);
    assert(kbm_usb_session_poll(&session,true,false,1)==KBM_USB_SESSION_SUSPEND);
    assert(kbm_usb_session_poll(&session,false,false,2)==(KBM_USB_SESSION_RESET|KBM_USB_SESSION_MOUNT));
    assert(kbm_usb_session_poll(&session,true,false,2)==KBM_USB_SESSION_MOUNT);
    std::array<uint8_t,32> keys{};std::array<uint8_t,8> report{};
    kbm_hid_key_set(keys.data(),0xe3,true);kbm_hid_keyboard_report(keys.data(),report.data());
    assert(report[0]==8&&report[2]==0); // Left Windows/GUI is a modifier, not normal6KRO slot.
    kbm_hid_key_set(keys.data(),0xe3,false);kbm_hid_key_set(keys.data(),0xe7,true);
    kbm_hid_keyboard_report(keys.data(),report.data());assert(report[0]==128);
    keys.fill(0);
    for(auto key:{0xe0,0xe2,0x4c})kbm_hid_key_set(keys.data(),key,true);
    kbm_hid_keyboard_report(keys.data(),report.data());assert(report[0]==5&&report[2]==0x4c);
    for(auto key:{0x4c,0xe2,0xe0}){kbm_hid_key_set(keys.data(),key,false);kbm_hid_keyboard_report(keys.data(),report.data());}
    for(auto value:report)assert(value==0);
    for(unsigned n=4;n<11;n++)kbm_hid_key_set(keys.data(),n,true);
    kbm_hid_keyboard_report(keys.data(),report.data());for(unsigned n=2;n<8;n++)assert(report[n]==1);
    kbm_hid_key_set(keys.data(),10,false);kbm_hid_keyboard_report(keys.data(),report.data());assert(report[2]==4&&report[7]==9);
    for(auto op:{0x11,0x12,0x13,0x22,0x7f})assert(!kbm_hid_op_can_wake(op));
    for(auto op:{0x10,0x20,0x21})assert(kbm_hid_op_can_wake(op));
    assert(!kbm_hid_resume_needed(false,5,5)); // Power/UP does not change the generation.
    assert(kbm_hid_resume_needed(false,6,5)); // New DOWN behind neutral reports wakes.
    assert(kbm_hid_resume_needed(false,0,UINT32_MAX));
    assert(kbm_hid_resume_needed(true,5,5));
    assert(kbm_hid_power_current(10,10,true));
    assert(!kbm_hid_power_current(10,11,true));
    assert(!kbm_hid_power_current(10,10,false));
    std::puts("PASS actual keyboard report builder: GUI modifiers, Ctrl+Alt+Del, reverse releases,6KRO; power/UP/release never auto-wake");
}
