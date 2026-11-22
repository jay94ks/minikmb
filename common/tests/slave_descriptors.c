#include "tusb.h"
#include "usb_descriptors.h"
#include "hid_controls.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

uint8_t tud_network_mac_address[6];
void pico_get_unique_board_id_string(char *id, size_t len) { snprintf(id,len,"MOCK"); }
int main(void) {
    assert(kbm_usb_function_mask()==0);
    for (uint8_t mask=0;mask<4;mask++) {
        kbm_usb_set_function_mask(mask);
        const uint8_t *config=tud_descriptor_configuration_cb(0);
        const tusb_desc_device_t *device=(const void *)tud_descriptor_device_cb();
        assert(device->bcdDevice==(0x0800 | mask));
        uint16_t length=(uint16_t)(config[2] | config[3]<<8), at=0;
        unsigned interfaces=0, msc=0, ncm=0, endpoints=0;
        for (;at<length;at+=config[at]) {
            assert(config[at]>=2 && at+config[at]<=length);
            if (config[at+1]==TUSB_DESC_INTERFACE) {
                const uint8_t *itf=config+at;
                if (itf[3]==0) assert(itf[2]==interfaces++);
                if (itf[5]==TUSB_CLASS_MSC) { ++msc; assert(itf[2]==4); }
                if (itf[5]==TUSB_CLASS_CDC && itf[6]==0x0d) {
                    ++ncm; assert(itf[2]==((mask&2)?5:4));
                }
            } else if (config[at+1]==TUSB_DESC_ENDPOINT) {
                unsigned ep=config[at+2];
                unsigned bit=(ep&15)+((ep&128)?16:0);
                assert(!(endpoints&(1u<<bit))); endpoints|=1u<<bit;
            }
        }
        assert(at==length && interfaces==config[4]);
        assert(msc==!!(mask&2) && ncm==!!(mask&1));
        assert(interfaces==4u+!!(mask&2)+2u*!!(mask&1));
    }
    const uint8_t *report=tud_hid_descriptor_report_cb(0);
    const uint8_t *configuration=tud_descriptor_configuration_cb(0);
    // Real HID descriptor length (base HID starts immediately after config).
    const unsigned report_length=configuration[9+9+7] | configuration[9+9+8]<<8;
    unsigned page=0,usage=0,id=0,depth=0,system=0,top=0,system_usages=0,count=0,bits=0;
    for(unsigned at=0;at<report_length;) {
        const uint8_t prefix=report[at++];assert(prefix!=0xfe);
        const unsigned size=(prefix&3)==3?4:(prefix&3);assert(at+size<=report_length);
        unsigned value=0;for(unsigned n=0;n<size;n++)value|=(unsigned)report[at++]<<(8*n);
        if(prefix==0x05)page=value;
        if(prefix==0x09) {
            usage=value;
            if(system==1) assert(kbm_hid_system_index(value)==(int)system_usages++);
        }
        if(prefix==0x95)count=value;
        if(prefix==0x75)bits=value;
        if(prefix==0x85)id=value;
        if(prefix==0xa1) { if(depth++==0){++top;if(page==1&&usage==0x80)system=1;} }
        if(prefix==0xc0){assert(depth);--depth;}
        if(system&&prefix==0x81&&page==1&&id==KBM_REPORT_ID_SYSTEM) { assert(value==2&&count==40&&bits==1);system=2; }
    }
    assert(top==3&&system==2&&depth==0&&system_usages==40);
    puts("PASS actual USB descriptors: four layouts, unique endpoints, keyboard/mouse/System Control collections,40-bit report3");
}
