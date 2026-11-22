/* Included once by each TinyUSB instance, after its namespace header. */
#include "storage_service.h"
#include "tusb.h"
#include <string.h>

_Static_assert(STORAGE_RAW_IO_BYTES == CFG_TUD_MSC_EP_BUFSIZE, "raw disk buffer size");

static struct {
  uint32_t seen_generation, io_generation, ticket;
  void *read_buffer;
  bool prevent_eject;
} raw_msc;

static bool RawSense(uint8_t lun, uint8_t key, uint8_t asc, uint8_t qualifier)
{
  (void)tud_msc_set_sense(lun, key, asc, qualifier);
  return false;
}

static bool RawReady(uint8_t lun, StorageRawMedia *media)
{
  *media = StorageService_RawStatus(RAW_PATH);
  if (media->generation != raw_msc.seen_generation) {
    raw_msc.seen_generation = media->generation;
    raw_msc.prevent_eject = false;
    if (media->sectors != 0U) return RawSense(lun, SCSI_SENSE_UNIT_ATTENTION, 0x28, 0);
  }
  return media->sectors != 0U || RawSense(lun, SCSI_SENSE_NOT_READY, 0x3A, 0);
}

static uint32_t RawInquiry(scsi_inquiry_resp_t *response, uint32_t size)
{
  if (size < sizeof(*response)) return 0;
  response->peripheral_device_type = 0;
  response->is_removable = 1;
  response->version = 5;
  memcpy(response->vendor_id, "MiniKMB ", 8);
  memcpy(response->product_id, "Virtual Disk    ", 16);
  memcpy(response->product_rev, "1.0 ", 4);
  return sizeof(*response);
}

static bool RawTestReady(uint8_t lun)
{
  StorageRawMedia media;
  return RawReady(lun, &media);
}

static void RawCapacity(uint8_t lun, uint32_t *sectors, uint16_t *block_size)
{
  StorageRawMedia media;
  *sectors = RawReady(lun, &media) ? media.sectors : 0;
  *block_size = STORAGE_RAW_BLOCK_BYTES;
}

static bool RawWritable(void)
{
  return StorageService_RawStatus(RAW_PATH).writable;
}

static bool RawValidate(uint8_t lun, uint32_t lba, uint16_t blocks, uint32_t bytes, bool write)
{
  if (bytes != (uint32_t)blocks * STORAGE_RAW_BLOCK_BYTES) {
    return RawSense(lun, SCSI_SENSE_ILLEGAL_REQUEST, 0x24, 0);
  }
  StorageRawMedia media;
  if (!RawReady(lun, &media)) return false;
  if (lba >= media.sectors || blocks > media.sectors - lba) {
    return RawSense(lun, SCSI_SENSE_ILLEGAL_REQUEST, 0x21, 0);
  }
  if (write && !media.writable) return RawSense(lun, SCSI_SENSE_DATA_PROTECT, 0x27, 0);
  raw_msc.io_generation = media.generation;
  return true;
}

static int32_t RawSubmit(uint8_t lun, uint32_t lba, uint32_t offset,
                         void *buffer, uint32_t bytes, bool write)
{
  if (offset != 0U || bytes == 0U || bytes % STORAGE_RAW_BLOCK_BYTES != 0U) {
    (void)RawSense(lun, SCSI_SENSE_ILLEGAL_REQUEST, 0x24, 0);
    return TUD_MSC_RET_ERROR;
  }
  const uint32_t ticket = tud_msc_io_ticket(lun);
  if (ticket == 0U || !StorageService_RawSubmit(RAW_PATH, raw_msc.io_generation, lba, bytes,
                                                write ? buffer : NULL)) {
    (void)RawSense(lun, SCSI_SENSE_NOT_READY, 0x04, 0);
    return TUD_MSC_RET_ERROR;
  }
  raw_msc.ticket = ticket;
  raw_msc.read_buffer = write ? NULL : buffer;
  return TUD_MSC_RET_ASYNC;
}

static bool RawPrevent(uint8_t lun, uint8_t prohibit)
{
  if (prohibit > 1U) return RawSense(lun, SCSI_SENSE_ILLEGAL_REQUEST, 0x24, 0);
  raw_msc.prevent_eject = prohibit != 0U;
  return true;
}

static bool RawStartStop(uint8_t lun, bool start, bool eject)
{
  if (start) return RawTestReady(lun);
  if (!eject) return true;
  const StorageRawMedia media = StorageService_RawStatus(RAW_PATH);
  if (raw_msc.prevent_eject && media.generation == raw_msc.seen_generation) {
    return RawSense(lun, SCSI_SENSE_ILLEGAL_REQUEST, 0x53, 0x02);
  }
  return media.sectors == 0U || StorageService_RawEject(RAW_PATH, media.generation);
}

static int32_t RawScsi(uint8_t lun, const uint8_t cmd[16], void *buffer, uint16_t capacity)
{
  /* Every WRITE(10) is synced by the storage worker before its success response. */
  if (cmd[0] == 0x35) return RawTestReady(lun) ? 0 : TUD_MSC_RET_ERROR;
  uint8_t reply[28] = {0};
  uint16_t bytes, allocation;
  if (cmd[0] == SCSI_CMD_INQUIRY && (cmd[1] & 1U) && cmd[2] == 0) {
    reply[3] = 1;
    bytes = 5;
    allocation = cmd[4];
  } else if (cmd[0] == SCSI_CMD_MODE_SENSE_6 || cmd[0] == 0x5A) {
    const uint8_t page = cmd[2] & 0x3FU;
    const bool long_mode = cmd[0] == 0x5A;
    if (page != 0 && page != 0x08U && page != 0x3FU) return TUD_MSC_RET_ERROR;
    const uint8_t header = long_mode ? 8U : 4U;
    bytes = header;
    if (page != 0U) {
      reply[header] = 0x08;  /* Caching page, WCE clear (write-through). */
      reply[header + 1U] = 18;
      bytes += 20;
    }
    reply[long_mode ? 1U : 0U] = (uint8_t)(bytes - (long_mode ? 2U : 1U));
    reply[long_mode ? 3U : 2U] = RawWritable() ? 0 : 0x80;
    allocation = long_mode ? (uint16_t)(((uint16_t)cmd[7] << 8) | cmd[8]) : cmd[4];
  } else {
    return TUD_MSC_RET_ERROR;
  }
  if (capacity > allocation) capacity = allocation;
  if (bytes > capacity) bytes = capacity;
  memcpy(buffer, reply, bytes);
  return (int32_t)bytes;
}

static void RawPoll(uint8_t lun, bool configured)
{
  if (!configured) raw_msc.prevent_eject = false;
  const uint32_t ticket = raw_msc.ticket;
  const bool current = configured && ticket != 0U && tud_msc_io_ticket(lun) == ticket;
  size_t bytes;
  StorageDiskResult status;
  bool write;
  if (!StorageService_RawTake(RAW_PATH, current ? raw_msc.io_generation : 0U,
                              current ? raw_msc.read_buffer : NULL, &bytes, &status, &write)) return;
  raw_msc.ticket = 0;
  if (!current) return;
  if (status == STORAGE_DISK_IO_ERROR) {
    (void)RawSense(lun, SCSI_SENSE_MEDIUM_ERROR, write ? 0x0C : 0x11, 0);
  } else if (status != STORAGE_DISK_OK) {
    (void)RawSense(lun, SCSI_SENSE_NOT_READY, 0x3A, 0);
  }
  (void)tud_msc_finish_io(lun, ticket, status == STORAGE_DISK_OK ? (int32_t)bytes : TUD_MSC_RET_ERROR);
}
