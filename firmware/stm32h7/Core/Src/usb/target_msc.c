#include "target_features.h"
#if MINIKMB_TARGET_MSC
#include "target_tusb_namespace.h"
#include "target_msc.h"
#include "optical_extents.h"
#include "storage_service.h"
#include "tusb.h"

#define RAW_PATH STORAGE_RAW_TARGET
#include "raw_msc.inc"

enum {
  GET_CONFIGURATION = 0x46, GET_EVENT_STATUS = 0x4A, READ_TOC = 0x43,
  READ_DISC_INFORMATION = 0x51, MODE_SENSE_10 = 0x5A, SET_CD_SPEED = 0xBB,
  SYNCHRONIZE_CACHE = 0x35,
  PROFILE_CD_ROM = 0x0008, PROFILE_DVD_ROM = 0x0010,
};

static uint32_t attention_generation, event_generation, read_generation, read_ticket;
static bool prevent_eject;

_Static_assert(STORAGE_OPTICAL_READ_BYTES == CFG_TUD_MSC_EP_BUFSIZE, "optical read buffer size");

static void Put16(uint8_t *target, uint16_t value)
{
  target[0] = (uint8_t)(value >> 8);
  target[1] = (uint8_t)value;
}

static void Put32(uint8_t *target, uint32_t value)
{
  Put16(target, (uint16_t)(value >> 16));
  Put16(target + 2, (uint16_t)value);
}

static uint16_t Get16(const uint8_t *source)
{
  return (uint16_t)(((uint16_t)source[0] << 8) | source[1]);
}

static int32_t Reply(const uint8_t *data, size_t bytes, void *buffer, uint16_t capacity, uint16_t alloc)
{
  if (capacity > alloc) capacity = alloc;
  if (bytes > capacity) bytes = capacity;
  memcpy(buffer, data, bytes);
  return (int32_t)bytes;
}

static bool Sense(uint8_t lun, uint8_t key, uint8_t asc, uint8_t qualifier)
{
  (void)tud_msc_set_sense(lun, key, asc, qualifier);
  return false;
}

static bool Ready(uint8_t lun, StorageOpticalMedia *media)
{
  StorageService_OpticalStatus(media);
  if (media->generation != attention_generation) {
    attention_generation = media->generation;
    if (media->bytes != 0U) return Sense(lun, SCSI_SENSE_UNIT_ATTENTION, 0x28, 0);
  }
  return media->bytes != 0U || Sense(lun, SCSI_SENSE_NOT_READY, 0x3A, 0);
}

enum { OPTICAL_LUN = 0 };
static bool optical_visible = true;
bool TargetMsc_OpticalVisible(void) { return optical_visible; }
void TargetMsc_SetOpticalVisible(bool enabled) { optical_visible = enabled; read_ticket = 0; prevent_eject = false; }
static uint8_t RawLun(void) { return optical_visible ? 1U : 0U; }

uint8_t tud_msc_get_maxlun_cb(void) { return optical_visible ? 2U : 1U; }

uint32_t tud_msc_inquiry2_cb(uint8_t lun, scsi_inquiry_resp_t *response, uint32_t size)
{
  if (lun == RawLun()) return RawInquiry(response, size);
  if (size < sizeof(*response)) return 0;
  response->peripheral_device_type = SCSI_PDT_CD_DVD;
  response->is_removable = 1;
  response->version = 5;
  memcpy(response->vendor_id, "MiniKMB ", 8);
  memcpy(response->product_id, "Virtual Optical ", 16);
  memcpy(response->product_rev, "1.0 ", 4);
  return sizeof(*response);
}

bool tud_msc_test_unit_ready_cb(uint8_t lun)
{
  if (lun == RawLun()) return RawTestReady(lun);
  StorageOpticalMedia media;
  return Ready(lun, &media);
}

void tud_msc_capacity_cb(uint8_t lun, uint32_t *blocks, uint16_t *block_size)
{
  StorageOpticalMedia media;
  if (lun == RawLun()) {
    RawCapacity(lun, blocks, block_size);
    return;
  }
  *blocks = Ready(lun, &media) ? (uint32_t)(media.bytes / OPTICAL_BLOCK_BYTES) : 0U;
  *block_size = OPTICAL_BLOCK_BYTES;
}

bool tud_msc_is_writable_cb(uint8_t lun)
{
  return lun == RawLun() && RawWritable();
}

bool tud_msc_read10_validate_cb(uint8_t lun, uint32_t lba, uint16_t blocks, uint32_t bytes)
{
  if (lun == RawLun()) return RawValidate(lun, lba, blocks, bytes, false);
  if (bytes != (uint32_t)blocks * OPTICAL_BLOCK_BYTES) {
    return Sense(lun, SCSI_SENSE_ILLEGAL_REQUEST, 0x24, 0);
  }
  StorageOpticalMedia media;
  if (!Ready(lun, &media)) return false;
  const uint32_t count = (uint32_t)(media.bytes / OPTICAL_BLOCK_BYTES);
  if (lba >= count || blocks > count - lba) return Sense(lun, SCSI_SENSE_ILLEGAL_REQUEST, 0x21, 0);
  read_generation = media.generation;
  return true;
}

int32_t tud_msc_read10_cb(uint8_t lun, uint32_t lba, uint32_t offset, void *buffer, uint32_t bytes)
{
  if (lun == RawLun()) return RawSubmit(lun, lba, offset, buffer, bytes, false);
  const uint64_t pos = (uint64_t)lba * OPTICAL_BLOCK_BYTES + offset;
  if (!StorageService_OpticalRead(read_generation, pos, bytes)) {
    (void)Sense(lun, SCSI_SENSE_NOT_READY, 0x04, 0x00);
    return TUD_MSC_RET_ERROR;
  }
  read_ticket = tud_msc_io_ticket(lun);
  return TUD_MSC_RET_ASYNC;
}

bool tud_msc_write10_validate_cb(uint8_t lun, uint32_t lba, uint16_t blocks, uint32_t bytes)
{
  if (lun == RawLun()) return RawValidate(lun, lba, blocks, bytes, true);
  return Sense(lun, SCSI_SENSE_DATA_PROTECT, 0x27, 0);
}

int32_t tud_msc_write10_cb(uint8_t lun, uint32_t lba, uint32_t offset, uint8_t *buffer, uint32_t bytes)
{
  if (lun == RawLun()) return RawSubmit(lun, lba, offset, buffer, bytes, true);
  (void)Sense(lun, SCSI_SENSE_DATA_PROTECT, 0x27, 0);
  return TUD_MSC_RET_ERROR;
}

bool tud_msc_prevent_allow_medium_removal_cb(uint8_t lun, uint8_t prohibit, uint8_t control)
{
  (void)control;
  if (lun == RawLun()) return RawPrevent(lun, prohibit);
  prevent_eject = prohibit != 0U;
  return true;
}

bool tud_msc_start_stop_cb(uint8_t lun, uint8_t power, bool start, bool eject)
{
  (void)power;
  if (lun == RawLun()) return RawStartStop(lun, start, eject);
  if (!eject) return true;
  if (start) {
    StorageOpticalMedia media;
    return Ready(lun, &media);
  }
  if (prevent_eject) return Sense(lun, SCSI_SENSE_ILLEGAL_REQUEST, 0x53, 0x02);
  StorageOpticalMedia media;
  StorageService_OpticalStatus(&media);
  return media.bytes == 0U || StorageService_OpticalEject();
}

static int32_t InquiryVpd(const uint8_t *cmd, void *buffer, uint16_t capacity)
{
  uint8_t reply[40] = {SCSI_PDT_CD_DVD, cmd[2]};
  size_t bytes;
  if (cmd[2] == 0) {
    reply[3] = 3;
    reply[4] = 0;
    reply[5] = 0x80;
    reply[6] = 0x83;
    bytes = 7;
  } else if (cmd[2] == 0x80 || cmd[2] == 0x83) {
    const uint16_t *serial = tud_descriptor_string_cb(3, 0x0409);
    if (serial == NULL || (serial[0] & 0xFFU) != 50U) return TUD_MSC_RET_ERROR;
    if (cmd[2] == 0x80) {
      reply[3] = 24;
      bytes = 28;
    } else {
      reply[3] = 36;
      reply[4] = 2; // ASCII code set
      reply[5] = 1; // T10 vendor identification
      reply[7] = 32;
      memcpy(reply + 8, "MiniKMB ", 8);
      bytes = 40;
    }
    for (unsigned i = 0; i < 24; ++i) reply[bytes - 24 + i] = (uint8_t)serial[i + 1];
  } else {
    return TUD_MSC_RET_ERROR;
  }
  return Reply(reply, bytes, buffer, capacity, Get16(cmd + 3));
}

static int32_t ModeSense(const uint8_t *cmd, void *buffer, uint16_t capacity)
{
  const uint8_t page = cmd[2] & 0x3FU;
  if (cmd[3] != 0U || (cmd[2] >> 6) == 3U ||
      (page != 0U && page != 0x2AU && page != 0x3FU)) return TUD_MSC_RET_ERROR;
  const bool long_format = cmd[0] == MODE_SENSE_10;
  uint8_t reply[36] = {0};
  size_t length = long_format ? 8U : 4U;
  if (page != 0U) {
    reply[length + 0] = 0x2A; // CD/DVD capabilities, data only.
    reply[length + 1] = 0x1A;
    if ((cmd[2] >> 6) != 1U) {
      reply[length + 2] = 0x08; // DVD-ROM read
      reply[length + 6] = 0x21; // tray model and locking
    }
    length += 28U;
  }
  if (long_format) {
    Put16(reply, (uint16_t)(length - 2U));
    reply[3] = 0x80;
  } else {
    reply[0] = (uint8_t)(length - 1U);
    reply[2] = 0x80;
  }
  return Reply(reply, length, buffer, capacity, long_format ? Get16(cmd + 7) : cmd[4]);
}

static size_t AddFeature(uint8_t *output, uint16_t start, uint8_t type, uint16_t feature,
                         uint8_t flags, const uint8_t *data, uint8_t length)
{
  if (feature < start || (type == 1 && (flags & 1U) == 0U) ||
      (type == 2 && feature != start)) return 0U;
  Put16(output, feature);
  output[2] = flags;
  output[3] = length;
  memcpy(output + 4, data, length);
  return 4U + length;
}

static int32_t GetConfiguration(const uint8_t *cmd, void *buffer, uint16_t capacity)
{
  const uint8_t type = cmd[1] & 3U;
  if (type == 3U) return TUD_MSC_RET_ERROR;
  StorageOpticalMedia media;
  StorageService_OpticalStatus(&media);
  uint16_t profile = 0;
  if (media.bytes != 0U) {
    profile = media.bytes / OPTICAL_BLOCK_BYTES <= 360000U ? PROFILE_CD_ROM : PROFILE_DVD_ROM;
  }
  uint8_t reply[64] = {0};
  Put16(reply + 6, profile);
  const uint16_t start = Get16(cmd + 2);
  size_t length = 8;
  const uint8_t profiles[] = {0, PROFILE_CD_ROM, profile == PROFILE_CD_ROM, 0,
                              0, PROFILE_DVD_ROM, profile == PROFILE_DVD_ROM, 0};
  const uint8_t core[] = {0, 0, 0, 0}; // Physical interface unspecified.
  const uint8_t morphing[] = {0, 0, 0, 0}; // GET EVENT uses polling.
  const uint8_t removable[] = {0x29, 0, 0, 0};
  const uint8_t random_read[] = {0, 0, 8, 0, 0, 1, 1, 0};
  length += AddFeature(reply + length, start, type, 0x0000, 3, profiles, sizeof(profiles));
  length += AddFeature(reply + length, start, type, 0x0001, 3, core, sizeof(core));
  length += AddFeature(reply + length, start, type, 0x0002, 3, morphing, sizeof(morphing));
  length += AddFeature(reply + length, start, type, 0x0003, 3, removable, sizeof(removable));
  length += AddFeature(reply + length, start, type, 0x0010, profile != 0, random_read, sizeof(random_read));
  Put32(reply, (uint32_t)(length - 4U));
  return Reply(reply, length, buffer, capacity, Get16(cmd + 7));
}

static int32_t GetEvent(const uint8_t *cmd, void *buffer, uint16_t capacity)
{
  if ((cmd[1] & 1U) == 0U) return TUD_MSC_RET_ERROR;
  const bool media_requested = (cmd[4] & 0x10U) != 0U;
  uint8_t reply[8] = {0, media_requested ? 6 : 2, media_requested ? 4 : 0x80, 0x10};
  if (media_requested) {
    StorageOpticalMedia media;
    StorageService_OpticalStatus(&media);
    reply[5] = media.bytes != 0U ? 2U : 0U;
    if (event_generation != media.generation) {
      reply[4] = media.bytes != 0U ? 2U : 3U;
      event_generation = media.generation;
    }
  }
  return Reply(reply, media_requested ? 8 : 4, buffer, capacity, Get16(cmd + 7));
}

static void TocAddress(uint8_t *address, uint32_t lba, bool msf)
{
  if (!msf) {
    Put32(address, lba);
    return;
  }
  const uint32_t frames = lba + 150U;
  address[1] = (uint8_t)(frames / 4500U);
  address[2] = (uint8_t)((frames / 75U) % 60U);
  address[3] = (uint8_t)(frames % 75U);
}

static int32_t ReadToc(uint8_t lun, const uint8_t *cmd, void *buffer, uint16_t capacity)
{
  StorageOpticalMedia media;
  if (!Ready(lun, &media)) return TUD_MSC_RET_ERROR;
  const uint8_t format = cmd[2] & 0x0FU;
  const bool msf = (cmd[1] & 2U) != 0U;
  const uint32_t leadout = (uint32_t)(media.bytes / OPTICAL_BLOCK_BYTES);
  if (msf && leadout >= 255U * 4500U) return TUD_MSC_RET_ERROR;
  uint8_t reply[20] = {0, 18, 1, 1, 0, 0x14, 1};
  uint16_t length;
  if (format == 1 || (format == 0 && cmd[6] <= 1)) {
    TocAddress(reply + 8, 0, msf);
    if (format == 1) {
      length = 12;
    } else {
      reply[13] = 0x14;
      reply[14] = 0xAA;
      TocAddress(reply + 16, leadout, msf);
      length = 20;
    }
  } else if (format == 0 && cmd[6] == 0xAA) {
    reply[6] = 0xAA;
    TocAddress(reply + 8, leadout, msf);
    length = 12;
  } else {
    return TUD_MSC_RET_ERROR;
  }
  Put16(reply, (uint16_t)(length - 2U));
  return Reply(reply, length, buffer, capacity, Get16(cmd + 7));
}

int32_t tud_msc_scsi_cb(uint8_t lun, const uint8_t cmd[16], void *buffer, uint16_t capacity)
{
  if (lun == RawLun()) return RawScsi(lun, cmd, buffer, capacity);
  switch (cmd[0]) {
    case SCSI_CMD_INQUIRY: return InquiryVpd(cmd, buffer, capacity);
    case SCSI_CMD_MODE_SENSE_6:
    case MODE_SENSE_10: return ModeSense(cmd, buffer, capacity);
    case GET_CONFIGURATION: return GetConfiguration(cmd, buffer, capacity);
    case GET_EVENT_STATUS: return GetEvent(cmd, buffer, capacity);
    case READ_TOC: return ReadToc(lun, cmd, buffer, capacity);
    case READ_DISC_INFORMATION: {
      StorageOpticalMedia media;
      if (!Ready(lun, &media)) return TUD_MSC_RET_ERROR;
      if ((cmd[1] & 7U) != 0U) return TUD_MSC_RET_ERROR;
      const uint8_t reply[34] = {0, 32, 0x0E, 1, 1, 1, 1};
      return Reply(reply, sizeof(reply), buffer, capacity, Get16(cmd + 7));
    }
    case SYNCHRONIZE_CACHE:
    case SET_CD_SPEED: return 0;
    case 0x2E: // WRITE AND VERIFY
    case 0xAA: // WRITE(12)
      (void)Sense(lun, SCSI_SENSE_DATA_PROTECT, 0x27, 0);
      return TUD_MSC_RET_ERROR;
    default: return TUD_MSC_RET_ERROR;
  }
}

void TargetMsc_Poll(bool configured)
{
  RawPoll(RawLun(), configured);
  if (!optical_visible) return;
  if (!configured) prevent_eject = false;
  const uint32_t ticket = read_ticket;
  void *buffer = configured && ticket != 0U && tud_msc_io_ticket(OPTICAL_LUN) == ticket
               ? TargetMsc_ReadBuffer() : NULL;
  size_t bytes;
  StorageDiskResult status;
  if (!StorageService_OpticalTake(buffer, CFG_TUD_MSC_EP_BUFSIZE, &bytes, &status)) return;
  read_ticket = 0;
  if (buffer == NULL) return;
  if (status == STORAGE_DISK_IO_ERROR) {
    (void)Sense(0, SCSI_SENSE_MEDIUM_ERROR, 0x11, 0);
  } else if (status != STORAGE_DISK_OK) {
    (void)Sense(0, SCSI_SENSE_NOT_READY, 0x3A, 0);
  }
  (void)tud_msc_finish_io(OPTICAL_LUN, ticket, bytes != 0U ? (int32_t)bytes : TUD_MSC_RET_ERROR);
}
#endif
