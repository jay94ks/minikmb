#define RAW_PATH STORAGE_RAW_MANAGEMENT
#include "raw_msc.inc"

uint32_t tud_msc_inquiry2_cb(uint8_t lun, scsi_inquiry_resp_t *response, uint32_t size)
{
  (void)lun;
  return RawInquiry(response, size);
}

bool tud_msc_test_unit_ready_cb(uint8_t lun) { return RawTestReady(lun); }

void tud_msc_capacity_cb(uint8_t lun, uint32_t *blocks, uint16_t *block_size)
{
  RawCapacity(lun, blocks, block_size);
}

bool tud_msc_is_writable_cb(uint8_t lun) { (void)lun; return RawWritable(); }

bool tud_msc_read10_validate_cb(uint8_t lun, uint32_t lba, uint16_t blocks, uint32_t bytes)
{
  return RawValidate(lun, lba, blocks, bytes, false);
}

bool tud_msc_write10_validate_cb(uint8_t lun, uint32_t lba, uint16_t blocks, uint32_t bytes)
{
  return RawValidate(lun, lba, blocks, bytes, true);
}

int32_t tud_msc_read10_cb(uint8_t lun, uint32_t lba, uint32_t offset, void *buffer, uint32_t bytes)
{
  return RawSubmit(lun, lba, offset, buffer, bytes, false);
}

int32_t tud_msc_write10_cb(uint8_t lun, uint32_t lba, uint32_t offset, uint8_t *buffer, uint32_t bytes)
{
  return RawSubmit(lun, lba, offset, buffer, bytes, true);
}

bool tud_msc_prevent_allow_medium_removal_cb(uint8_t lun, uint8_t prohibit, uint8_t control)
{
  (void)control;
  return RawPrevent(lun, prohibit);
}

bool tud_msc_start_stop_cb(uint8_t lun, uint8_t power, bool start, bool eject)
{
  (void)power;
  return RawStartStop(lun, start, eject);
}

int32_t tud_msc_scsi_cb(uint8_t lun, const uint8_t cmd[16], void *buffer, uint16_t capacity)
{
  return RawScsi(lun, cmd, buffer, capacity);
}

void ManagementMsc_Poll(bool configured) { RawPoll(0, configured); }
