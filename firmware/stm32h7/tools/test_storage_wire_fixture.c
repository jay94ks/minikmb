/* Reuse the RAM card and FatFs used by the standalone storage tests. */
#define main StorageDisk_TestMain
#include "test_storage_disk.c"
#undef main
#include "target_tusb_namespace.h"
#include "target_msc.h"
#include "storage_service.h"
#include "tusb.h"

const uint8_t test_uid[12] = {0x70, 0x31, 0x29};
static uint8_t sense[3];
static int32_t optical_done;
static bool bot_pending;
static uint8_t bot_lun;
static uint32_t bot_ticket;
static uint8_t optical_reply[STORAGE_OPTICAL_READ_BYTES];

tusb_speed_t tud_speed_get(void) { return TUSB_SPEED_FULL; }
bool tud_msc_set_sense(uint8_t lun, uint8_t key, uint8_t asc, uint8_t ascq)
{
  (void)lun;
  sense[0] = key;
  sense[1] = asc;
  sense[2] = ascq;
  return true;
}
void *TargetMsc_ReadBuffer(void) { return bot_pending ? optical_reply : NULL; }
uint32_t tud_msc_io_ticket(uint8_t lun) { return lun == bot_lun && bot_pending ? bot_ticket : 0; }
bool tud_msc_finish_io(uint8_t lun, uint32_t ticket, int32_t bytes)
{
  if (ticket == 0 || tud_msc_io_ticket(lun) != ticket) return false;
  optical_done = bytes;
  bot_pending = false;
  return true;
}
const uint8_t *TestOptical_Reply(void) { return optical_reply; }
int32_t TestOptical_Done(void) { return optical_done; }
uint8_t TestOptical_Sense(void) { return sense[1]; }
void TestOptical_BotReset(void) { bot_pending = false; }
int32_t TestOptical_Read(uint32_t lba, uint16_t blocks, uint32_t bytes)
{
  optical_done = 0;
  if (!tud_msc_read10_validate_cb(0, lba, blocks, bytes)) return -1;
  ++bot_ticket;
  bot_lun = 0;
  bot_pending = true;
  const int32_t ret = tud_msc_read10_cb(0, lba, 0, optical_reply, bytes);
  bot_pending = ret == TUD_MSC_RET_ASYNC;
  return ret;
}
int32_t TestOptical_Command(const uint8_t *cmd, uint8_t *data, uint16_t bytes)
{
  return tud_msc_scsi_cb(0, cmd, data, bytes);
}
bool TestOptical_Eject(void)
{
  return tud_msc_start_stop_cb(0, 0, false, true);
}
#include "../Core/Src/storage_service.c"

static bool serial_enabled;
static uint64_t NextSession(void)
{
  static uint64_t id = 0;
  return ++id;
}
static uint8_t TargetState(void) { return 1; }
static void SetSerial(bool enabled) { serial_enabled = enabled; }
static void GetSerial(ManagementSerialStatus *status) { *status = (ManagementSerialStatus){0}; }
static uint16_t SendSerial(const uint8_t *data, uint16_t length)
{
  (void)data;
  (void)length;
  return serial_enabled ? 0 : 9;
}
static uint16_t ReadSerial(uint8_t *data, uint16_t length) { (void)data; (void)length; return 0; }

void TestStorage_Init(void)
{
  static const ManagementSerialBackend serial = {SetSerial, SendSerial, ReadSerial, GetSerial};
  card = calloc(SECTORS, BLOCK);
  assert(card != NULL);
  uint8_t mkfs_buffer[8192];
  assert(f_mkfs(SDPath, FM_FAT32 | FM_SFD, BLOCK, mkfs_buffer, sizeof(mkfs_buffer)) == FR_OK);
  StorageService_Start();
  CardChanged(true);
  const ManagementPlatform platform = {
    .new_session = NextSession, .target_state = TargetState, .serial = &serial,
    .storage = StorageService_Backend(), .optical = true,
  };
  const uint8_t controller[12] = {0x70, 0x31, 0x29};
  StorageService_SetUid(controller);
  Management_Init(&platform, 0);
}

void TestStorage_Free(void) { StorageDisk_Invalidate(); free(card); }
void ManagementMsc_Poll(bool configured);

bool TestStorage_Work(void)
{
  const bool result = Work();
  TargetMsc_Poll(true);
  ManagementMsc_Poll(true);
  return result;
}

#undef tud_msc_io_ticket
#undef tud_msc_finish_io
#undef tud_msc_set_sense
#undef tud_msc_read10_validate_cb
#undef tud_msc_write10_validate_cb
#undef tud_msc_read10_cb
#undef tud_msc_write10_cb

extern bool tud_msc_read10_validate_cb(uint8_t, uint32_t, uint16_t, uint32_t);
extern bool tud_msc_write10_validate_cb(uint8_t, uint32_t, uint16_t, uint32_t);
extern int32_t tud_msc_read10_cb(uint8_t, uint32_t, uint32_t, void *, uint32_t);
extern int32_t tud_msc_write10_cb(uint8_t, uint32_t, uint32_t, uint8_t *, uint32_t);

bool tud_msc_set_sense(uint8_t lun, uint8_t key, uint8_t asc, uint8_t ascq)
{
  return target_tud_msc_set_sense(lun, key, asc, ascq);
}
uint32_t tud_msc_io_ticket(uint8_t lun) { return target_tud_msc_io_ticket(lun); }
bool tud_msc_finish_io(uint8_t lun, uint32_t ticket, int32_t bytes)
{
  return target_tud_msc_finish_io(lun, ticket, bytes);
}

bool TestRaw_Validate(bool target, bool write, uint32_t lba, uint16_t blocks, uint32_t bytes)
{
  if (target) {
    return write ? target_tud_msc_write10_validate_cb(1, lba, blocks, bytes)
                 : target_tud_msc_read10_validate_cb(1, lba, blocks, bytes);
  }
  return write ? tud_msc_write10_validate_cb(0, lba, blocks, bytes)
               : tud_msc_read10_validate_cb(0, lba, blocks, bytes);
}

int32_t TestRaw_Transfer(bool target, bool write, uint32_t lba, uint8_t *buffer, uint32_t bytes)
{
  optical_done = 0;
  ++bot_ticket;
  bot_lun = target ? 1 : 0;
  bot_pending = true;
  int32_t ret;
  if (target) {
    ret = write ? target_tud_msc_write10_cb(1, lba, 0, buffer, bytes)
                : target_tud_msc_read10_cb(1, lba, 0, buffer, bytes);
  } else {
    ret = write ? tud_msc_write10_cb(0, lba, 0, buffer, bytes)
                : tud_msc_read10_cb(0, lba, 0, buffer, bytes);
  }
  bot_pending = ret == TUD_MSC_RET_ASYNC;
  return ret;
}
bool TestStorage_SerialEnabled(void) { return serial_enabled; }
void TestStorage_Present(bool present) { inserted = present; CardChanged(present); }
