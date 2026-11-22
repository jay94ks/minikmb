#include "disk_bridge.h"
#include "app_state.h"
#include "protocol.h"
#include "network_bridge.h"
#include "firmware/disk_reader.hpp"
#include "pico/stdlib.h"
#include "tusb.h"
extern "C" void kbm_msc_resume_read(void);
namespace {
kbm::DiskReader disk;
uint32_t now() { return to_ms_since_boot(get_absolute_time()); }
}
bool disk_bridge_visible(void) { return disk.ready(now()); }
void disk_bridge_task(void) {
    kbm_packet_t packet;
    // One small fragment per loop; HID starts first in the owning USB loop.
    if(queue_try_remove(&g_disk_commands,&packet)) {
        if(packet.op==KBM_DISK_CONFIG) disk.configure(packet.payload,packet.len,now());
        else if(packet.op==KBM_DISK_DATA) disk.accept(packet.payload,packet.len);
        else if(packet.op==KBM_DISK_FAIL) disk.fail(packet.payload,packet.len);
    }
    static kbm_disk_transfer_t transfer;
    if(queue_try_remove(&g_disk_transfers,&transfer)) disk.accept(transfer.payload,transfer.len);
    __atomic_store_n(&g_disk_flags,(disk.enabled()?1u:0u)|(disk.ready(now())?2u:0u),__ATOMIC_RELEASE);
    if(disk.request(now(),packet) && queue_try_add(&g_disk_events,&packet)) disk.published(now());
    if(network_bridge_usb_available() && tud_ready()) kbm_msc_resume_read();
}
extern "C" void tud_msc_inquiry_cb(uint8_t, uint8_t vendor[8],uint8_t product[16],uint8_t revision[4]) {
    memcpy(vendor,"KBM     ",8); memcpy(product,"Folder Snapshot ",16); memcpy(revision,"1.00",4);
}
extern "C" bool tud_msc_test_unit_ready_cb(uint8_t lun) {
    if(!disk.ready(now())) { tud_msc_set_sense(lun,SCSI_SENSE_NOT_READY,0x3a,0); return false; }
    if(disk.takeAttention()) { tud_msc_set_sense(lun,SCSI_SENSE_UNIT_ATTENTION,0x28,0); return false; }
    return true;
}
extern "C" void tud_msc_capacity_cb(uint8_t,uint32_t* count,uint16_t* size) { *count=disk.ready(now())?disk.sectors():0; *size=512; }
extern "C" bool tud_msc_is_writable_cb(uint8_t) { return false; }
extern "C" bool tud_msc_start_stop_cb(uint8_t,uint8_t,bool start,bool eject) { if(!start && eject) disk.eject(); return true; }
extern "C" int32_t tud_msc_read10_cb(uint8_t,uint32_t lba,uint32_t offset,void* buffer,uint32_t size) {
    return disk.read(now(),lba,offset,buffer,size);
}
extern "C" int32_t tud_msc_write10_cb(uint8_t lun,uint32_t,uint32_t,uint8_t*,uint32_t) {
    tud_msc_set_sense(lun,SCSI_SENSE_DATA_PROTECT,0x27,0); return -1;
}
extern "C" int32_t tud_msc_scsi_cb(uint8_t lun,const uint8_t command[16],void*,uint16_t) {
    if(command[0]==0x35) return 0; // SYNCHRONIZE CACHE: read-only, nothing to flush.
    tud_msc_set_sense(lun,SCSI_SENSE_ILLEGAL_REQUEST,0x20,0); return -1;
}
