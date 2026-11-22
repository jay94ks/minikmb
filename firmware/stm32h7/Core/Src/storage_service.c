#include "storage_service.h"
#include "storage_disk.h"
#include "optical_extents.h"
#include "writable_disk.h"

#include "main.h"
#include "stm32h7xx.h"
#include "FreeRTOS.h"
#include "task.h"
#include "cmsis_os2.h"

#include <stdatomic.h>
#include <string.h>

enum {
  NO_CARD, IDLE, PREPARING, UPLOADING, VERIFYING, IMAGE_READY,
  MOUNTING, SERVING, UNMOUNTING, STORAGE_ERROR, FORMATTING = MANAGEMENT_STORAGE_FORMATTING,
  READ_FREE = 0, READ_QUEUED, READ_RUNNING, READ_DONE, READ_COPYING,
  JOB_RUNNING = 0, JOB_SUCCESS, JOB_FAILED, JOB_CANCELLED,
  JOB_BEGIN = 1, JOB_COMMIT, JOB_MOUNT, JOB_UNMOUNT, JOB_CARD_INIT, JOB_USER_WRITE,
  JOB_DISK_CREATE, JOB_DISK_COMMIT, JOB_DISK_OPEN, JOB_DISK_CLOSE, JOB_DISK_READ, JOB_DISK_WRITE, JOB_DISK_SYNC, JOB_CARD_FORMAT, JOB_DISK_LIST, JOB_DISK_DELETE, JOB_CHANNEL_NAME,
  JOB_STATUS = 0x0007, JOB_CANCEL = 0x0008, UPLOAD_CANCEL = 0x0009,
  UPLOAD_BEGIN = 0x0301, UPLOAD_DATA, UPLOAD_COMMIT, UPLOAD_ABORT, IMAGE_INFO, IMAGE_MOUNT, IMAGE_UNMOUNT,
  CARD_INFO, CARD_INIT, CARD_USER_READ, CARD_USER_BEGIN, CARD_USER_DATA, CARD_USER_COMMIT, CARD_USER_ABORT, CARD_FORMAT = MANAGEMENT_CARD_FORMAT,
  DISK_CREATE = 0x0310, DISK_COMMIT, DISK_OPEN, DISK_CLOSE, DISK_READ, DISK_WRITE, DISK_RESULT, DISK_SYNC, DISK_INFO,
  EMULATION_INFO = 0x0319, OPTICAL_SET, DISK_LIST, DISK_LIST_RESULT, DISK_DELETE, CHANNEL_NAME_GET, CHANNEL_NAME_SET,
  RAW_CLOSED = 0, RAW_PENDING, RAW_PROVISIONING, RAW_DIRECT, RAW_TARGET, RAW_MANAGEMENT,
  CARD_FAT32 = 1, CARD_EXFAT = 2,
  BUSY = 4, CHUNK_SIZE = 1008, JOB_COUNT = 8,
  UPLOAD_BLOCK_BYTES = 2 * 1024, UPLOAD_BLOCKS = 32, UPLOAD_BLOCKS_PER_WRITE = 8,
};

typedef struct {
  uint64_t owner, done, total;
  uint32_t id, object_id, detail;
  uint16_t result;
  uint8_t state, kind;
  bool event_pending;
} StorageJob;

static atomic_bool optical_enabled = true, optical_applied = true;
bool StorageService_OpticalEnabled(void) { return atomic_load(&optical_enabled); }
void StorageService_OpticalApplied(bool enabled) { atomic_store(&optical_applied, enabled); }
static struct { WritableDiskInfo items[4]; uint32_t cursor, next, job, remove_id; uint64_t owner; uint8_t count; } catalog;

static StaticTask_t task_control;
static StackType_t task_stack[16384U / sizeof(StackType_t)]
    __attribute__((section(".rtos_stacks"), aligned(32)));
static TaskHandle_t worker;
typedef struct {
  const uint8_t *data;
  uint16_t used;
} UploadBlock;

_Static_assert(UPLOAD_BLOCK_BYTES % 512 == 0 && UPLOAD_BLOCKS % UPLOAD_BLOCKS_PER_WRITE == 0,
               "upload blocks require sector aligned writes");
static uint8_t upload_pages[UPLOAD_BLOCKS][UPLOAD_BLOCK_BYTES]
    __attribute__((section(".axi_upload"), aligned(32)));
static UploadBlock upload_ring[UPLOAD_BLOCKS];
_Static_assert(ATOMIC_INT_LOCK_FREE == 2, "upload cursors must be lock free");
static atomic_uint upload_published, upload_released;
static size_t upload_partial;
static uint8_t next_job_slot;
static uint64_t upload_owner;
static uint32_t last_id;
static bool abort_requested;
static bool format_exfat;
static StorageJob *active_job;
static StorageJob jobs[JOB_COUNT];
static ManagementStorageStatus current;
static StorageDiskImage image;
static uint32_t media_generation;
static StorageCardProfile card_profile, pending_profile;
static uint8_t channel_name[STORAGE_CHANNEL_NAME_BYTES], pending_name[STORAGE_CHANNEL_NAME_BYTES];
static uint16_t channel_name_length, pending_name_length;
static StorageDiskResult name_status = STORAGE_DISK_NOT_READY;
static uint8_t controller_uid[STORAGE_CONTROLLER_UID_BYTES];
static StorageDiskResult profile_status = STORAGE_DISK_NOT_READY;
static StorageDiskResult user_status = STORAGE_DISK_NOT_READY;
static uint8_t user_values[2][STORAGE_USER_ID_BYTES]
    __attribute__((section(".axi_card_data"), aligned(32)));
static struct {
  WritableDiskInfo info;
  uint64_t owner;
  uint32_t lease, pending_lba, expected_crc, read_job;
  uint8_t state;
  StorageRawPath path;
  bool writable, close_requested;
  uint8_t sector[WRITABLE_DISK_SECTOR];
} raw_disk;
_Static_assert((unsigned)STORAGE_RAW_BLOCK_BYTES == WRITABLE_DISK_SECTOR, "raw sector size");
static struct {
  uint32_t generation, lba;
  size_t bytes;
  StorageDiskResult status;
  StorageRawPath path;
  uint8_t state;
  bool write;
} raw_io;
static uint8_t raw_usb_buffer[STORAGE_RAW_IO_BYTES]
    __attribute__((section(".axi_raw_usb"), aligned(32)));
static uint8_t user_slot;
static uint16_t user_length;
static uint32_t user_revision;
static struct {
  uint64_t owner;
  uint32_t id;
  uint16_t bytes, received;
} user_transfer;
static struct {
  uint64_t offset;
  size_t bytes;
  uint32_t generation;
  StorageDiskResult status;
  uint8_t state;
} optical_read;
static uint8_t read_buffer[STORAGE_OPTICAL_READ_BYTES]
    __attribute__((section(".axi_optical"), aligned(32)));

static void SetStorageState(uint8_t next)
{
  if ((current.state == SERVING) != (next == SERVING)) ++media_generation;
  current.state = next;
}

static uint64_t ReadLE(const uint8_t *bytes, size_t n)
{
  uint64_t value = 0;
  for (size_t i = 0; i < n; ++i) value |= (uint64_t)bytes[i] << (8U * i);
  return value;
}

static void WriteLE(uint8_t *bytes, uint64_t value, size_t n)
{
  for (size_t i = 0; i < n; ++i) {
    bytes[i] = (uint8_t)value;
    value >>= 8;
  }
}

static void EncodeJob(const StorageJob *job, uint8_t *result)
{
  WriteLE(result, job->id, 4);
  result[4] = job->state;
  result[5] = job->kind;
  WriteLE(result + 6, job->result, 2);
  WriteLE(result + 8, job->detail, 4);
  WriteLE(result + 12, job->object_id, 4);
  WriteLE(result + 16, job->done, 8);
  WriteLE(result + 24, job->total, 8);
}

/* The caller owns the service lock during metadata, queue and job transitions. */
static uint32_t StartJob(uint8_t kind, uint64_t owner, uint64_t total)
{
  configASSERT(active_job == NULL && last_id != UINT32_MAX);
  active_job = &jobs[next_job_slot];
  next_job_slot = (uint8_t)((next_job_slot + 1U) % JOB_COUNT);
  if (active_job->event_pending && active_job->owner == owner) ++current.dropped_events;
  *active_job = (StorageJob){.id = ++last_id, .owner = owner, .kind = kind, .total = total};
  current.job_id = active_job->id;
  return current.job_id;
}

static void CompleteJob(StorageDiskResult result, uint32_t object_id)
{
  if (active_job == NULL) return;
  active_job->result = (uint16_t)result;
  active_job->state = result == STORAGE_DISK_OK ? JOB_SUCCESS :
                      (result == STORAGE_DISK_CANCELLED ? JOB_CANCELLED : JOB_FAILED);
  active_job->detail = result == STORAGE_DISK_FRAGMENT_LIMIT ? 4096U :
                       (result == STORAGE_DISK_NOT_READY ? 2U : 0U);
  active_job->object_id = object_id;
  active_job->event_pending = active_job->owner != 0U;
  current.job_id = 0;
  active_job = NULL;
}

static void EndUpload(void)
{
  atomic_store_explicit(&upload_published, 0U, memory_order_relaxed);
  atomic_store_explicit(&upload_released, 0U, memory_order_relaxed);
  upload_partial = 0U;
  abort_requested = false;
  current.upload_id = 0;
  upload_owner = 0;
}

static void FailUpload(StorageDiskResult result)
{
  StorageDisk_Invalidate();
  const bool inserted = StorageDisk_Present();
  taskENTER_CRITICAL();
  CompleteJob(result, 0);
  EndUpload();
  current.image_id = 0;
  const uint8_t next = result == STORAGE_DISK_CANCELLED ? IDLE : STORAGE_ERROR;
  SetStorageState(inserted ? next : NO_CARD);
  current.last_error = (uint16_t)result;
  taskEXIT_CRITICAL();
}

static bool TrackRead(void *context, uint64_t done, uint64_t total)
{
  (void)context;
  (void)total;
  taskENTER_CRITICAL();
  active_job->done = done;
  const bool proceed = !abort_requested;
  taskEXIT_CRITICAL();
  return proceed;
}

static bool ContinueRecovery(void *context, uint64_t done, uint64_t total)
{
  (void)context;
  (void)done;
  (void)total;
  taskENTER_CRITICAL();
  const bool proceed = current.state == IDLE;
  taskEXIT_CRITICAL();
  return proceed;
}

static void ForgetCardData(void)
{
  profile_status = user_status = name_status = STORAGE_DISK_NOT_READY;
  channel_name_length = 0;
  user_length = 0;
  user_transfer.id = 0;
}

static bool RawExported(void)
{
  return raw_disk.state == RAW_TARGET || raw_disk.state == RAW_MANAGEMENT;
}

StorageRawMedia StorageService_RawStatus(StorageRawPath path)
{
  StorageRawMedia media = {0};
  taskENTER_CRITICAL();
  if (RawExported() && raw_disk.path == path && !raw_disk.close_requested) {
    media = (StorageRawMedia){raw_disk.info.sectors, raw_disk.lease, raw_disk.writable};
  }
  taskEXIT_CRITICAL();
  return media;
}

bool StorageService_RawSubmit(StorageRawPath path, uint32_t generation, uint32_t lba,
                              size_t bytes, const void *write_data)
{
  if (bytes == 0U || bytes > sizeof(raw_usb_buffer) || bytes % WRITABLE_DISK_SECTOR != 0U) {
    return false;
  }
  taskENTER_CRITICAL();
  const bool valid = RawExported() && raw_disk.path == path && raw_disk.lease == generation &&
                     !raw_disk.close_requested && raw_io.state == READ_FREE &&
                     lba < raw_disk.info.sectors &&
                     bytes / WRITABLE_DISK_SECTOR <= raw_disk.info.sectors - lba &&
                     (write_data == NULL || raw_disk.writable);
  if (valid) {
    raw_io.generation = generation;
    raw_io.lba = lba;
    raw_io.bytes = bytes;
    raw_io.path = path;
    raw_io.write = write_data != NULL;
    raw_io.state = READ_COPYING;
  }
  taskEXIT_CRITICAL();
  if (!valid) return false;

  /* Pin this buffer until the USB task publishes the completed copy. */
  if (write_data != NULL) memcpy(raw_usb_buffer, write_data, bytes);

  taskENTER_CRITICAL();
  const bool current_lease = RawExported() && raw_disk.path == path &&
                             raw_disk.lease == generation && !raw_disk.close_requested;
  raw_io.state = current_lease ? READ_QUEUED : READ_FREE;
  taskEXIT_CRITICAL();
  if (current_lease) xTaskNotifyGive(worker);
  return current_lease;
}

bool StorageService_RawTake(StorageRawPath path, uint32_t generation, void *read_data,
                            size_t *bytes, StorageDiskResult *status, bool *write)
{
  taskENTER_CRITICAL();
  if (raw_io.path != path || raw_io.state != READ_DONE) {
    taskEXIT_CRITICAL();
    return false;
  }
  *status = raw_io.generation == generation && raw_disk.lease == generation
          ? raw_io.status : STORAGE_DISK_NOT_READY;
  *bytes = raw_io.bytes;
  *write = raw_io.write;
  const bool copy = !*write && read_data != NULL && *status == STORAGE_DISK_OK;
  raw_io.state = copy ? READ_COPYING : READ_FREE;
  taskEXIT_CRITICAL();
  if (!copy) return true;

  memcpy(read_data, raw_usb_buffer, *bytes);

  taskENTER_CRITICAL();
  if (raw_disk.lease != generation) *status = STORAGE_DISK_NOT_READY;
  raw_io.state = READ_FREE;
  taskEXIT_CRITICAL();
  return true;
}

bool StorageService_RawEject(StorageRawPath path, uint32_t generation)
{
  taskENTER_CRITICAL();
  const bool valid = RawExported() && raw_disk.path == path && raw_disk.lease == generation;
  if (valid) raw_disk.close_requested = true;
  taskEXIT_CRITICAL();
  if (valid) xTaskNotifyGive(worker);
  return valid;
}

static bool RunRawUsbIo(void)
{
  taskENTER_CRITICAL();
  const bool queued_io = raw_io.state == READ_QUEUED;
  if (queued_io) raw_io.state = READ_RUNNING;
  taskEXIT_CRITICAL();
  if (!queued_io) return false;

  const uint32_t sectors = (uint32_t)(raw_io.bytes / WRITABLE_DISK_SECTOR);
  StorageDiskResult result;
  if (raw_io.write) {
    result = WritableDisk_Write(raw_io.lba, sectors, raw_usb_buffer);
    if (result == STORAGE_DISK_OK) result = WritableDisk_Sync();
  } else {
    result = WritableDisk_Read(raw_io.lba, sectors, raw_usb_buffer);
  }

  taskENTER_CRITICAL();
  raw_io.status = result;
  raw_io.state = READ_DONE;
  if (result == STORAGE_DISK_IO_ERROR || result == STORAGE_DISK_NOT_READY) raw_disk.close_requested = true;
  taskEXIT_CRITICAL();
  return true;
}

static void ClearWritableDisk(void)
{
  if (raw_io.state == READ_QUEUED) {
    raw_io.status = STORAGE_DISK_NOT_READY;
    raw_io.state = READ_DONE;
  }
  raw_disk.owner = 0;
  raw_disk.lease = 0;
  raw_disk.read_job = 0;
  raw_disk.state = RAW_CLOSED;
  raw_disk.close_requested = false;
}

static void CardChanged(bool inserted)
{
  if (!inserted) {
    taskENTER_CRITICAL();
    const bool raw_open = raw_disk.state != RAW_CLOSED;
    taskEXIT_CRITICAL();
    if (raw_open) (void)WritableDisk_Close();
    taskENTER_CRITICAL();
    ClearWritableDisk();
    ForgetCardData();
    taskEXIT_CRITICAL();
    FailUpload(STORAGE_DISK_NOT_READY);
    return;
  }

  StorageCardProfile profile;
  size_t length = 0;
  const StorageDiskResult profile_result = StorageCard_EnsureProfile(controller_uid, &profile);
  size_t name_length = 0;
  StorageDiskResult loaded_name = profile_result;
  if (profile_result == STORAGE_DISK_OK) {
    loaded_name = memcmp(profile.controller_uid, controller_uid, sizeof(controller_uid)) == 0 ?
        StorageCard_LoadName(pending_name, sizeof(pending_name), &name_length) : STORAGE_DISK_BAD_STATE;
  }
  const StorageDiskResult user_result = StorageCard_LoadUserId(user_values[user_slot ^ 1U],
                                                              STORAGE_USER_ID_BYTES, &length);
  taskENTER_CRITICAL();
  profile_status = profile_result;
  name_status = loaded_name;
  channel_name_length = loaded_name == STORAGE_DISK_OK ? (uint16_t)name_length : 0U;
  if (channel_name_length != 0U) memcpy(channel_name, pending_name, channel_name_length);
  if (profile_result == STORAGE_DISK_OK) card_profile = profile;
  user_status = user_result;
  if (user_result == STORAGE_DISK_OK) {
    user_slot ^= 1U;
    user_length = (uint16_t)length;
    if (++user_revision == 0U) ++user_revision;
  }
  current.state = IDLE;
  current.last_error = 0;
  current.received = current.total = 0;
  taskEXIT_CRITICAL();

  StorageDiskImage recovered;
  const StorageDiskResult result = StorageDisk_Load(&recovered, ContinueRecovery, NULL);
  taskENTER_CRITICAL();
  if (current.state == IDLE) {
    if (result == STORAGE_DISK_OK && last_id != UINT32_MAX) {
      image = recovered;
      current.image_id = ++last_id;
      current.state = IMAGE_READY;
    } else if (result != STORAGE_DISK_NOT_FOUND) {
      current.last_error = result == STORAGE_DISK_OK ? STORAGE_DISK_BAD_STATE : (uint16_t)result;
      current.state = STORAGE_ERROR;
    }
  }
  taskEXIT_CRITICAL();
}

static void BeginUpload(void)
{
  /* The management task publishes PREPARING after validating the request. */
  const StorageDiskResult result = StorageDisk_Begin(&image);
  if (result != STORAGE_DISK_OK) {
    FailUpload(result);
    return;
  }

  taskENTER_CRITICAL();
  const bool cancelled = abort_requested;
  if (!cancelled) {
    current.state = UPLOADING;
    current.upload_id = active_job->id;
    CompleteJob(STORAGE_DISK_OK, current.upload_id);
  }
  taskEXIT_CRITICAL();
  if (cancelled) FailUpload(STORAGE_DISK_CANCELLED);
}

static void PublishUploadBlock(unsigned sequence, size_t used)
{
  const size_t slot = sequence % UPLOAD_BLOCKS;
  upload_ring[slot] = (UploadBlock){upload_pages[slot], (uint16_t)used};
  atomic_store_explicit(&upload_published, sequence + 1U, memory_order_release);
}

static bool WriteUploadBatch(uint8_t state)
{
  const unsigned released = atomic_load_explicit(&upload_released, memory_order_relaxed);
  const unsigned published = atomic_load_explicit(&upload_published, memory_order_acquire);
  const unsigned ready = published - released;
  if (ready == 0U || (state != VERIFYING && ready < UPLOAD_BLOCKS_PER_WRITE)) return false;

  const size_t head = released % UPLOAD_BLOCKS;
  const size_t contiguous = UPLOAD_BLOCKS - head;
  size_t count = ready < UPLOAD_BLOCKS_PER_WRITE ? ready : UPLOAD_BLOCKS_PER_WRITE;
  if (count > contiguous) count = contiguous;
  /* Pages follow ring order. Only the final published page can be short. */
  const size_t length = (count - 1U) * UPLOAD_BLOCK_BYTES + upload_ring[head + count - 1U].used;
  const StorageDiskResult result = StorageDisk_Write(upload_ring[head].data, length);
  if (result != STORAGE_DISK_OK) {
    FailUpload(result);
    return true;
  }

  /* The producer can reuse these pages only after FatFs and SD DMA complete. */
  atomic_store_explicit(&upload_released, released + (unsigned)count, memory_order_release);
  return true;
}

static void CommitUpload(void)
{
  const StorageDiskResult result = StorageDisk_Finish(TrackRead, NULL);
  if (result != STORAGE_DISK_OK) {
    FailUpload(result);
    return;
  }
  taskENTER_CRITICAL();
  current.image_id = active_job->id;
  current.state = IMAGE_READY;
  CompleteJob(STORAGE_DISK_OK, current.image_id);
  EndUpload();
  taskEXIT_CRITICAL();
}

static bool RunOpticalRead(void)
{
  taskENTER_CRITICAL();
  if (optical_read.state != READ_QUEUED) {
    taskEXIT_CRITICAL();
    return false;
  }
  optical_read.state = READ_RUNNING;
  const bool available = current.state == SERVING && optical_read.generation == media_generation;
  taskEXIT_CRITICAL();

  const StorageDiskResult status = available ?
      StorageDisk_Read(optical_read.offset, read_buffer, (uint32_t)(optical_read.bytes / OPTICAL_SD_SECTOR_BYTES)) :
      STORAGE_DISK_NOT_READY;
  const bool lost = available && (status == STORAGE_DISK_NOT_READY || status == STORAGE_DISK_IO_ERROR);
  const bool present = !lost || StorageDisk_Present();
  taskENTER_CRITICAL();
  if (lost) {
    /* The disk backend invalidated its extent map. A queued management state takes precedence. */
    current.image_id = 0;
    if (current.state == SERVING) SetStorageState(present ? STORAGE_ERROR : NO_CARD);
    current.last_error = (uint16_t)status;
  }
  optical_read.status = status;
  optical_read.state = READ_DONE;
  taskEXIT_CRITICAL();
  return true;
}

void StorageService_OpticalStatus(StorageOpticalMedia *media)
{
  const bool present = StorageDisk_Present();
  taskENTER_CRITICAL();
  media->bytes = current.state == SERVING && present ? image.size : 0;
  media->generation = media_generation;
  taskEXIT_CRITICAL();
}

bool StorageService_OpticalRead(uint32_t generation, uint64_t offset, size_t bytes)
{
  if (offset % OPTICAL_SD_SECTOR_BYTES != 0U || bytes == 0U || bytes > sizeof(read_buffer) ||
      bytes % OPTICAL_SD_SECTOR_BYTES != 0U || !StorageDisk_Present()) return false;
  taskENTER_CRITICAL();
  const bool accept = current.state == SERVING && media_generation == generation &&
                      optical_read.state == READ_FREE && offset < image.size &&
                      bytes <= image.size - offset;
  if (accept) {
    optical_read.offset = offset;
    optical_read.bytes = bytes;
    optical_read.generation = generation;
    optical_read.state = READ_QUEUED;
  }
  taskEXIT_CRITICAL();
  if (accept) xTaskNotifyGive(worker);
  return accept;
}

bool StorageService_OpticalTake(void *data, size_t capacity, size_t *bytes, StorageDiskResult *status)
{
  taskENTER_CRITICAL();
  if (optical_read.state != READ_DONE) {
    taskEXIT_CRITICAL();
    return false;
  }
  *status = optical_read.status;
  *bytes = 0;
  if (*status == STORAGE_DISK_OK &&
      (current.state != SERVING || optical_read.generation != media_generation || !StorageDisk_Present())) {
    *status = STORAGE_DISK_NOT_READY;
  }
  const size_t length = optical_read.bytes;
  const bool copy = *status == STORAGE_DISK_OK && data != NULL && capacity >= length;
  optical_read.state = copy ? READ_COPYING : READ_FREE;
  taskEXIT_CRITICAL();
  if (!copy) return true;

  memcpy(data, read_buffer, length);

  taskENTER_CRITICAL();
  if (current.state != SERVING || optical_read.generation != media_generation || !StorageDisk_Present()) {
    *status = STORAGE_DISK_NOT_READY;
  } else {
    *bytes = length;
  }
  optical_read.state = READ_FREE;
  taskEXIT_CRITICAL();
  return true;
}

bool StorageService_OpticalEject(void)
{
  taskENTER_CRITICAL();
  const bool ready = current.state == SERVING || current.state == IMAGE_READY;
  if (current.state == SERVING) SetStorageState(IMAGE_READY);
  taskEXIT_CRITICAL();
  return ready;
}

static bool RunMediaJob(void)
{
  taskENTER_CRITICAL();
  const uint8_t state = current.state;
  if (state != MOUNTING && state != UNMOUNTING) {
    taskEXIT_CRITICAL();
    return false;
  }
  const bool present = StorageDisk_Present() && current.image_id != 0U;
  const bool cancelled = abort_requested;
  const bool serving = cancelled ? state == UNMOUNTING : state == MOUNTING;
  if (!present) {
    SetStorageState(StorageDisk_Present() ? STORAGE_ERROR : NO_CARD);
    CompleteJob(STORAGE_DISK_NOT_READY, 0);
  } else if (cancelled) {
    SetStorageState(serving ? SERVING : IMAGE_READY);
    CompleteJob(STORAGE_DISK_CANCELLED, 0);
  } else {
    SetStorageState(serving ? SERVING : IMAGE_READY);
    CompleteJob(STORAGE_DISK_OK, current.image_id);
  }
  abort_requested = false;
  taskEXIT_CRITICAL();
  return true;
}

static void RunFormatJob(void)
{
  StorageDiskResult result = StorageDisk_Format(format_exfat);
  StorageCardProfile initialized;
  if (result == STORAGE_DISK_OK) result = StorageCard_EnsureProfile(controller_uid, &initialized);
  const bool present = StorageDisk_Present();
  taskENTER_CRITICAL();
  if (result == STORAGE_DISK_OK) {
    card_profile = initialized; profile_status = name_status = STORAGE_DISK_OK;
    channel_name_length = 0; user_status = STORAGE_DISK_NOT_FOUND;
  }
  SetStorageState(result == STORAGE_DISK_OK ? IDLE : (present ? STORAGE_ERROR : NO_CARD));
  current.last_error = (uint16_t)result;
  CompleteJob(result, result == STORAGE_DISK_OK ? active_job->id : 0U);
  taskEXIT_CRITICAL();
}

static bool RunCardJob(void)
{
  taskENTER_CRITICAL();
  const uint8_t kind = active_job == NULL ? 0 : active_job->kind;
  const bool cancelled = abort_requested;
  taskEXIT_CRITICAL();
  if (kind == JOB_CARD_FORMAT) {
    RunFormatJob();
    return true;
  }
  if (kind != JOB_CARD_INIT && kind != JOB_USER_WRITE && kind != JOB_CHANNEL_NAME) return false;

  StorageDiskResult result = STORAGE_DISK_CANCELLED;
  if (!cancelled) {
    result = kind == JOB_CHANNEL_NAME ? StorageCard_StoreName(controller_uid, pending_name, pending_name_length) :
        kind == JOB_CARD_INIT ? StorageCard_InitProfile(&pending_profile) :
        StorageCard_StoreUserId(user_values[user_slot ^ 1U], user_transfer.bytes);
  }
  taskENTER_CRITICAL();
  if (result == STORAGE_DISK_OK) {
    if (kind == JOB_CHANNEL_NAME) {
      channel_name_length = pending_name_length;
      memcpy(channel_name, pending_name, channel_name_length);
      name_status = STORAGE_DISK_OK;
    } else if (kind == JOB_CARD_INIT) {
      channel_name_length = 0; name_status = STORAGE_DISK_OK;
      card_profile = pending_profile;
      profile_status = STORAGE_DISK_OK;
    } else {
      user_slot ^= 1U;
      user_length = user_transfer.bytes;
      if (++user_revision == 0U) ++user_revision;
      user_status = STORAGE_DISK_OK;
      active_job->done = user_length;
    }
  }
  if (kind == JOB_USER_WRITE) user_transfer.id = 0;
  CompleteJob(result, result == STORAGE_DISK_OK ? active_job->id : 0U);
  abort_requested = false;
  taskEXIT_CRITICAL();
  return true;
}

static bool WritableJob(uint8_t kind)
{
  return kind >= JOB_DISK_CREATE && kind <= JOB_DISK_SYNC;
}

static bool RunWritableJob(void)
{
  taskENTER_CRITICAL();
  const uint8_t kind = active_job == NULL ? 0U : active_job->kind;
  const bool abort = abort_requested;
  const bool closing = raw_disk.close_requested;
  taskEXIT_CRITICAL();
  if (!WritableJob(kind)) {
    if (!closing || kind != 0U) return false;
    (void)WritableDisk_Close();
    taskENTER_CRITICAL();
    ClearWritableDisk();
    abort_requested = false;
    taskEXIT_CRITICAL();
    return true;
  }

  StorageDiskResult result = STORAGE_DISK_CANCELLED;
  WritableDiskInfo opened;
  if (!abort || kind == JOB_DISK_CLOSE) {
    switch (kind) {
      case JOB_DISK_CREATE: result = WritableDisk_Create(&raw_disk.info, TrackRead, NULL); break;
      case JOB_DISK_COMMIT: result = WritableDisk_Commit(raw_disk.expected_crc, TrackRead, NULL); break;
      case JOB_DISK_OPEN: result = WritableDisk_Open(raw_disk.info.id, raw_disk.writable, &opened); break;
      case JOB_DISK_CLOSE: result = WritableDisk_Close(); break;
      case JOB_DISK_READ: result = WritableDisk_Read(raw_disk.pending_lba, 1, raw_disk.sector); break;
      case JOB_DISK_WRITE: result = WritableDisk_Write(raw_disk.pending_lba, 1, raw_disk.sector); break;
      case JOB_DISK_SYNC: result = WritableDisk_Sync(); break;
    }
  } else if (kind == JOB_DISK_COMMIT) {
    (void)WritableDisk_Close();
  }

  taskENTER_CRITICAL();
  const uint32_t lease = raw_disk.lease;
  if (result == STORAGE_DISK_OK) {
    if (kind == JOB_DISK_CREATE) raw_disk.state = RAW_PROVISIONING;
    if (kind == JOB_DISK_OPEN) {
      raw_disk.info = opened;
      if (raw_disk.path == STORAGE_RAW_TARGET) raw_disk.state = RAW_TARGET;
      else if (raw_disk.path == STORAGE_RAW_MANAGEMENT) raw_disk.state = RAW_MANAGEMENT;
      else raw_disk.state = RAW_DIRECT;
    }
    if (kind == JOB_DISK_READ) raw_disk.read_job = active_job->id;
    active_job->done = active_job->total;
  } else if (result == STORAGE_DISK_IO_ERROR || result == STORAGE_DISK_NOT_READY) {
    raw_disk.close_requested = true;
  }
  if (kind == JOB_DISK_CLOSE || kind == JOB_DISK_COMMIT ||
      (result != STORAGE_DISK_OK && (kind == JOB_DISK_CREATE || kind == JOB_DISK_OPEN))) {
    ClearWritableDisk();
  }
  CompleteJob(result, result == STORAGE_DISK_OK ? lease : 0U);
  abort_requested = false;
  taskEXIT_CRITICAL();
  return true;
}

static bool RunCatalogJob(void)
{
  taskENTER_CRITICAL();
  const uint8_t kind = active_job == NULL ? 0U : active_job->kind;
  const bool cancelled = abort_requested;
  taskEXIT_CRITICAL();
  if (kind != JOB_DISK_LIST && kind != JOB_DISK_DELETE) return false;
  const StorageDiskResult result = cancelled ? STORAGE_DISK_CANCELLED :
      (kind == JOB_DISK_LIST ? WritableDisk_List(catalog.cursor, catalog.items, &catalog.count, &catalog.next) : WritableDisk_Delete(catalog.remove_id));
  taskENTER_CRITICAL();
  const uint32_t id = active_job->id;
  if (kind == JOB_DISK_LIST) catalog.job = result == STORAGE_DISK_OK ? id : 0U;
  CompleteJob(result, result == STORAGE_DISK_OK ? id : 0U);
  abort_requested = false;
  taskEXIT_CRITICAL();
  return true;
}

static bool Work(void)
{
  if (RunCatalogJob() || RunOpticalRead() || RunMediaJob() || RunCardJob() || RunRawUsbIo() || RunWritableJob()) return true;
  taskENTER_CRITICAL();
  const uint8_t state = current.state;
  const bool cancelled = abort_requested;
  taskEXIT_CRITICAL();
  if (cancelled) {
    FailUpload(STORAGE_DISK_CANCELLED);
    return true;
  }
  if (state == PREPARING) {
    BeginUpload();
    return true;
  }
  if (state != UPLOADING && state != VERIFYING) return false;

  if (WriteUploadBatch(state)) return true;
  if (state == VERIFYING) {
    CommitUpload();
    return true;
  }
  return false;
}

static void StorageTask(void *arg)
{
  (void)arg;
  bool previous = false;
  for (;;) {
    const bool inserted = StorageDisk_Present();
    if (inserted != previous) {
      CardChanged(inserted);
      previous = inserted;
      continue;
    }
    if (!Work()) {
      (void)ulTaskNotifyTake(pdTRUE, pdMS_TO_TICKS(10U));
    }
  }
}

static uint16_t BeginCommand(uint64_t owner, const StorageDiskImage *candidate,
                             uint8_t *result, size_t *length, uint32_t *detail)
{
  if (!StorageDisk_Present() || current.state == NO_CARD) {
    *detail = 2;
    return STORAGE_DISK_NOT_READY;
  }
  if (current.state == PREPARING || current.state == UPLOADING || current.state == VERIFYING || active_job != NULL ||
      raw_disk.state != RAW_CLOSED || last_id == UINT32_MAX) return STORAGE_DISK_BAD_STATE;
  image = *candidate;
  current.image_id = 0;
  current.received = 0;
  current.total = candidate->size;
  current.last_error = 0;
  SetStorageState(PREPARING);
  upload_owner = owner;
  WriteLE(result, StartJob(JOB_BEGIN, owner, candidate->size), 4);
  *length = 4;
  return STORAGE_DISK_OK;
}

static uint16_t DataCommand(uint64_t owner, const uint8_t *payload, size_t length,
                             uint8_t *result, size_t *result_length, uint32_t *detail)
{
  if (length <= 12U || length > 12U + CHUNK_SIZE) return STORAGE_DISK_BAD_PAYLOAD;
  const uint32_t id = (uint32_t)ReadLE(payload, 4);
  const uint64_t offset = ReadLE(payload + 4, 8);
  const size_t bytes = length - 12U;

  taskENTER_CRITICAL();
  if (current.state != UPLOADING || abort_requested || upload_owner != owner || id != current.upload_id) {
    *detail = current.state;
    taskEXIT_CRITICAL();
    return STORAGE_DISK_BAD_STATE;
  }
  const uint64_t remaining = current.total - current.received;
  if (offset != current.received || bytes != (remaining < CHUNK_SIZE ? remaining : CHUNK_SIZE)) {
    taskEXIT_CRITICAL();
    return STORAGE_DISK_RANGE;
  }
  const unsigned published = atomic_load_explicit(&upload_published, memory_order_relaxed);
  const unsigned released = atomic_load_explicit(&upload_released, memory_order_acquire);
  const size_t partial = upload_partial;
  const size_t free = (UPLOAD_BLOCKS - (published - released)) * UPLOAD_BLOCK_BYTES - partial;
  taskEXIT_CRITICAL();
  if (bytes > free) {
    *detail = 10U;
    return BUSY;
  }

  /* Only this producer can write unpublished pages. Do not mask interrupts while copying. */
  const size_t room = UPLOAD_BLOCK_BYTES - partial;
  const size_t first = bytes < room ? bytes : room;
  memcpy(upload_pages[published % UPLOAD_BLOCKS] + partial, payload + 12, first);
  if (first != bytes) memcpy(upload_pages[(published + 1U) % UPLOAD_BLOCKS], payload + 12 + first, bytes - first);

  const bool full = first == room;
  taskENTER_CRITICAL();
  if (current.state != UPLOADING || abort_requested || upload_owner != owner ||
      id != current.upload_id || offset != current.received) {
    *detail = current.state;
    taskEXIT_CRITICAL();
    return STORAGE_DISK_BAD_STATE;
  }
  if (full) PublishUploadBlock(published, UPLOAD_BLOCK_BYTES);
  upload_partial = full ? bytes - first : partial + first;
  current.received += bytes;
  const uint64_t accepted = current.received;
  taskEXIT_CRITICAL();

  if (full && published + 1U - released >= UPLOAD_BLOCKS_PER_WRITE) xTaskNotifyGive(worker);
  WriteLE(result, accepted, 8);
  *result_length = 8;
  return STORAGE_DISK_OK;
}

static uint16_t JobCommand(uint64_t owner, uint16_t opcode, uint32_t id,
                            uint8_t *result, size_t *length)
{
  for (unsigned i = 0; i < JOB_COUNT; ++i) {
    if (jobs[i].id != id) continue;
    if (opcode == JOB_STATUS) {
      EncodeJob(&jobs[i], result);
      *length = 32;
    } else if (&jobs[i] == active_job) {
      if (jobs[i].owner != owner || jobs[i].kind == JOB_CARD_FORMAT) return STORAGE_DISK_BAD_STATE;
      abort_requested = true;
    }
    return STORAGE_DISK_OK;
  }
  return STORAGE_DISK_NOT_FOUND;
}

static uint16_t ImageInfo(uint8_t *result, size_t *length)
{
  if (current.image_id == 0U) return STORAGE_DISK_NOT_FOUND;
  memset(result, 0, 48);
  WriteLE(result, current.image_id, 4);
  WriteLE(result + 4, image.size, 8);
  result[12] = 1;
  result[13] = current.state == SERVING;
  WriteLE(result + 14, image.name_length, 2);
  WriteLE(result + 16, image.crc32, 4);
  memcpy(result + 48, image.name, image.name_length);
  *length = 48U + image.name_length;
  return STORAGE_DISK_OK;
}

static uint16_t OtherCommand(uint64_t owner, uint16_t opcode, const uint8_t *payload,
                              size_t length, uint8_t *result, size_t *result_length)
{
  if (opcode == IMAGE_INFO) {
    return length == 0U ? ImageInfo(result, result_length) : STORAGE_DISK_BAD_PAYLOAD;
  }
  if (opcode != JOB_STATUS && opcode != JOB_CANCEL && opcode != UPLOAD_CANCEL &&
      opcode != UPLOAD_ABORT && opcode != UPLOAD_COMMIT &&
      opcode != IMAGE_MOUNT && opcode != IMAGE_UNMOUNT) return STORAGE_DISK_UNSUPPORTED;
  if (length != 4U) return STORAGE_DISK_BAD_PAYLOAD;
  const uint32_t id = (uint32_t)ReadLE(payload, 4);
  if (id == 0U) return STORAGE_DISK_NOT_FOUND;
  if (opcode == JOB_STATUS || opcode == JOB_CANCEL) {
    return JobCommand(owner, opcode, id, result, result_length);
  }
  if (opcode == IMAGE_MOUNT || opcode == IMAGE_UNMOUNT) {
    if (id != current.image_id) return STORAGE_DISK_NOT_FOUND;
    const bool mount = opcode == IMAGE_MOUNT;
    const uint8_t expected = mount ? IMAGE_READY : SERVING;
    if (current.state != expected || active_job != NULL ||
        raw_disk.state == RAW_PROVISIONING || last_id == UINT32_MAX) return STORAGE_DISK_BAD_STATE;
    SetStorageState(mount ? MOUNTING : UNMOUNTING);
    WriteLE(result, StartJob(mount ? JOB_MOUNT : JOB_UNMOUNT, owner, 0), 4);
    *result_length = 4;
    return STORAGE_DISK_OK;
  }
  if (id != current.upload_id || owner != upload_owner) return STORAGE_DISK_NOT_FOUND;
  if (opcode != UPLOAD_COMMIT) {
    abort_requested = true;
    return STORAGE_DISK_OK;
  }
  if (current.state != UPLOADING || abort_requested || current.received != current.total ||
      last_id == UINT32_MAX) return STORAGE_DISK_BAD_STATE;
  if (upload_partial != 0U) {
    const unsigned published = atomic_load_explicit(&upload_published, memory_order_relaxed);
    PublishUploadBlock(published, upload_partial);
    upload_partial = 0U;
  }
  current.state = VERIFYING;
  WriteLE(result, StartJob(JOB_COMMIT, owner, current.total), 4);
  *result_length = 4;
  return STORAGE_DISK_OK;
}

static bool CardIdle(void)
{
  return active_job == NULL && raw_disk.state == RAW_CLOSED && (current.state == IDLE || current.state == IMAGE_READY ||
                                current.state == STORAGE_ERROR);
}

static uint16_t CardInfo(uint8_t *result, size_t *length)
{
  const bool present = StorageDisk_Present();
  const bool loaded = present && current.state != NO_CARD;
  const bool profiled = loaded && profile_status == STORAGE_DISK_OK;
  const bool has_user = loaded && user_status == STORAGE_DISK_OK;
  memset(result, 0, 52);
  result[0] = (uint8_t)((present ? 1U : 0U) | (profiled ? 2U : 0U) | (has_user ? 4U : 0U));
  if (profiled) memcpy(result + 8, &card_profile, sizeof(card_profile));
  if (has_user) {
    WriteLE(result + 2, user_length, 2);
    WriteLE(result + 4, user_revision, 4);
  }
  memcpy(result + 36, controller_uid, sizeof(controller_uid));
  WriteLE(result + 48, present ? profile_status : STORAGE_DISK_NOT_READY, 2);
  WriteLE(result + 50, present ? user_status : STORAGE_DISK_NOT_READY, 2);
  *length = 52;
  return STORAGE_DISK_OK;
}

static uint16_t UserRead(const uint8_t *payload, size_t n, uint8_t *result, size_t *length)
{
  if (n != 8U) return STORAGE_DISK_BAD_PAYLOAD;
  if (user_status != STORAGE_DISK_OK) return (uint16_t)user_status;
  if (ReadLE(payload, 4) != user_revision) return STORAGE_DISK_BAD_STATE;
  const size_t offset = (size_t)ReadLE(payload + 4, 2);
  const size_t bytes = (size_t)ReadLE(payload + 6, 2);
  if (bytes == 0U || bytes > 256U || offset >= user_length || bytes > user_length - offset) {
    return STORAGE_DISK_RANGE;
  }
  memcpy(result, user_values[user_slot] + offset, bytes);
  *length = bytes;
  return STORAGE_DISK_OK;
}

static uint16_t UserUpdate(uint64_t owner, uint16_t opcode, const uint8_t *payload, size_t n,
                           uint8_t *result, size_t *length)
{
  if (n < 4U) return STORAGE_DISK_BAD_PAYLOAD;
  if (user_transfer.id == 0U || user_transfer.owner != owner ||
      ReadLE(payload, 4) != user_transfer.id) return STORAGE_DISK_NOT_FOUND;
  if (opcode == CARD_USER_DATA) {
    if (n <= 6U || n > 6U + CHUNK_SIZE) return STORAGE_DISK_BAD_PAYLOAD;
    if (!CardIdle()) return STORAGE_DISK_BAD_STATE;
    const size_t bytes = n - 6U;
    if (ReadLE(payload + 4, 2) != user_transfer.received ||
        bytes > (size_t)(user_transfer.bytes - user_transfer.received)) return STORAGE_DISK_RANGE;
    memcpy(user_values[user_slot ^ 1U] + user_transfer.received, payload + 6, bytes);
    user_transfer.received += (uint16_t)bytes;
    WriteLE(result, user_transfer.received, 2);
    *length = 2;
    return STORAGE_DISK_OK;
  }
  if (n != 4U) return STORAGE_DISK_BAD_PAYLOAD;
  if (opcode == CARD_USER_ABORT) {
    if (active_job != NULL && active_job->kind == JOB_USER_WRITE) abort_requested = true;
    else user_transfer.id = 0;
    return STORAGE_DISK_OK;
  }
  if (!CardIdle() || user_transfer.received != user_transfer.bytes || last_id == UINT32_MAX) {
    return STORAGE_DISK_BAD_STATE;
  }
  WriteLE(result, StartJob(JOB_USER_WRITE, owner, user_transfer.bytes), 4);
  *length = 4;
  return STORAGE_DISK_OK;
}

static uint16_t CardCommand(uint64_t owner, uint16_t opcode, const uint8_t *payload, size_t n,
                            uint8_t *result, size_t *length, uint32_t *detail)
{
  if (opcode == CARD_INFO) return n == 0U ? CardInfo(result, length) : STORAGE_DISK_BAD_PAYLOAD;
  if (!StorageDisk_Present() || current.state == NO_CARD) {
    *detail = 2;
    return STORAGE_DISK_NOT_READY;
  }
  if (opcode == CARD_USER_READ) return UserRead(payload, n, result, length);
  if (opcode == CARD_FORMAT) {
    if (n != 4U || (payload[0] != CARD_FAT32 && payload[0] != CARD_EXFAT) ||
        payload[1] != 0U || payload[2] != 0U || payload[3] != 0U) return STORAGE_DISK_BAD_PAYLOAD;
    if (!CardIdle() || user_transfer.id != 0U || last_id == UINT32_MAX) return STORAGE_DISK_BAD_STATE;
    format_exfat = payload[0] == CARD_EXFAT;
    ForgetCardData();
    current.image_id = 0;
    current.received = current.total = 0;
    current.last_error = 0;
    SetStorageState(FORMATTING);
    WriteLE(result, StartJob(JOB_CARD_FORMAT, owner, 0), 4);
    *length = 4;
    return STORAGE_DISK_OK;
  }
  if (opcode == CARD_INIT) {
    if (n != STORAGE_BOARD_ID_BYTES) return STORAGE_DISK_BAD_PAYLOAD;
    if (!CardIdle() || last_id == UINT32_MAX) return STORAGE_DISK_BAD_STATE;
    memcpy(pending_profile.board_id, payload, STORAGE_BOARD_ID_BYTES);
    memcpy(pending_profile.controller_uid, controller_uid, sizeof(controller_uid));
    WriteLE(result, StartJob(JOB_CARD_INIT, owner, 0), 4);
    *length = 4;
    return STORAGE_DISK_OK;
  }
  if (opcode == CARD_USER_BEGIN) {
    if (n != 2U) return STORAGE_DISK_BAD_PAYLOAD;
    const uint16_t bytes = (uint16_t)ReadLE(payload, 2);
    if (bytes > STORAGE_USER_ID_BYTES) return STORAGE_DISK_RANGE;
    if (!CardIdle() || user_transfer.id != 0U || last_id == UINT32_MAX) return STORAGE_DISK_BAD_STATE;
    user_transfer.id = ++last_id;
    user_transfer.owner = owner;
    user_transfer.bytes = bytes;
    user_transfer.received = 0;
    WriteLE(result, user_transfer.id, 4);
    *length = 4;
    return STORAGE_DISK_OK;
  }
  return UserUpdate(owner, opcode, payload, n, result, length);
}

static uint16_t WritableOpenCommand(uint64_t owner, uint16_t opcode, const uint8_t *payload,
                                    size_t n, uint8_t *result, size_t *length)
{
  const bool create = opcode == DISK_CREATE;
  if ((create && n != 12U) || (!create && n != 8U)) return STORAGE_DISK_BAD_PAYLOAD;
  const size_t flags = create ? 8U : 4U;
  const uint8_t path = payload[flags + 1U];
  if (ReadLE(payload + flags + 2U, 2) != 0U ||
      (create ? path != 0U : (path > STORAGE_RAW_MANAGEMENT || payload[flags] > 1U))) {
    return STORAGE_DISK_BAD_PAYLOAD;
  }
  const uint32_t id = (uint32_t)ReadLE(payload, 4);
  const uint32_t sectors = create ? (uint32_t)ReadLE(payload + 4, 4) : 0U;
  if (id == 0U || id > WRITABLE_DISK_MAX_ID || (create && sectors == 0U)) return STORAGE_DISK_RANGE;
  if (raw_disk.state != RAW_CLOSED || active_job != NULL || last_id >= UINT32_MAX - 1U ||
      (current.state != IDLE && current.state != IMAGE_READY && current.state != STORAGE_ERROR &&
       (create || current.state != SERVING))) return STORAGE_DISK_BAD_STATE;
  raw_disk.info = (WritableDiskInfo){.id = id, .sectors = sectors,
                                     .format = create ? (WritableDiskFormat)payload[flags] : 0};
  raw_disk.state = RAW_PENDING;
  raw_disk.path = (StorageRawPath)path;
  raw_disk.owner = owner;
  raw_disk.lease = ++last_id;
  raw_disk.writable = create || payload[flags] != 0U;
  const uint32_t job = StartJob(create ? JOB_DISK_CREATE : JOB_DISK_OPEN, owner,
                                (uint64_t)sectors * WRITABLE_DISK_SECTOR);
  WriteLE(result, job, 4);
  *length = 4;
  return STORAGE_DISK_OK;
}

static uint16_t WritableInfo(uint64_t owner, size_t n, uint8_t *result, size_t *length)
{
  if (n != 0U) return STORAGE_DISK_BAD_PAYLOAD;
  if (raw_disk.state == RAW_CLOSED) return STORAGE_DISK_NOT_FOUND;
  if ((!RawExported() && raw_disk.owner != owner) || raw_disk.close_requested) return STORAGE_DISK_BAD_STATE;
  WriteLE(result, raw_disk.info.id, 4);
  WriteLE(result + 4, raw_disk.info.sectors, 4);
  WriteLE(result + 8, raw_disk.lease, 4);
  result[12] = (uint8_t)raw_disk.info.format;
  result[13] = raw_disk.state;
  result[14] = raw_disk.writable;
  result[15] = 0;
  *length = 16;
  return STORAGE_DISK_OK;
}

static uint16_t WritableCommand(uint64_t owner, uint16_t opcode, const uint8_t *payload, size_t n,
                                uint8_t *result, size_t *length, uint32_t *detail)
{
  if (!StorageDisk_Present() || current.state == NO_CARD) {
    *detail = 2U;
    return STORAGE_DISK_NOT_READY;
  }
  if (opcode == DISK_CREATE || opcode == DISK_OPEN) {
    return WritableOpenCommand(owner, opcode, payload, n, result, length);
  }
  if (opcode == DISK_INFO) return WritableInfo(owner, n, result, length);
  if (raw_disk.state == RAW_CLOSED || raw_disk.close_requested ||
      (RawExported() ? opcode != DISK_CLOSE : raw_disk.owner != owner)) {
    return STORAGE_DISK_BAD_STATE;
  }
  if (n < 4U || ReadLE(payload, 4) != raw_disk.lease) return STORAGE_DISK_BAD_PAYLOAD;
  if (opcode == DISK_RESULT) {
    if (n != 9U || payload[8] > 1U) return STORAGE_DISK_BAD_PAYLOAD;
    if (raw_disk.read_job == 0U || ReadLE(payload + 4, 4) != raw_disk.read_job) {
      return STORAGE_DISK_NOT_FOUND;
    }
    memcpy(result, raw_disk.sector + 256U * payload[8], 256U);
    *length = 256U;
    return STORAGE_DISK_OK;
  }
  if (active_job != NULL || last_id == UINT32_MAX) return STORAGE_DISK_BAD_STATE;
  uint8_t kind;
  uint64_t total = 0;
  switch (opcode) {
    case DISK_COMMIT:
      if (n != 8U) return STORAGE_DISK_BAD_PAYLOAD;
      if (raw_disk.state != RAW_PROVISIONING) return STORAGE_DISK_BAD_STATE;
      raw_disk.expected_crc = (uint32_t)ReadLE(payload + 4, 4);
      total = (uint64_t)raw_disk.info.sectors * WRITABLE_DISK_SECTOR;
      kind = JOB_DISK_COMMIT;
      break;
    case DISK_READ:
    case DISK_WRITE: {
      const bool write = opcode == DISK_WRITE;
      if (n != (write ? 8U + WRITABLE_DISK_SECTOR : 8U)) return STORAGE_DISK_BAD_PAYLOAD;
      if (write && !raw_disk.writable) return STORAGE_DISK_READ_ONLY;
      const uint32_t lba = (uint32_t)ReadLE(payload + 4, 4);
      if (lba >= raw_disk.info.sectors) return STORAGE_DISK_RANGE;
      raw_disk.pending_lba = lba;
      raw_disk.read_job = 0;
      if (write) memcpy(raw_disk.sector, payload + 8, WRITABLE_DISK_SECTOR);
      total = WRITABLE_DISK_SECTOR;
      kind = write ? JOB_DISK_WRITE : JOB_DISK_READ;
      break;
    }
    case DISK_CLOSE:
    case DISK_SYNC:
      if (n != 4U) return STORAGE_DISK_BAD_PAYLOAD;
      if (opcode == DISK_CLOSE) raw_disk.close_requested = true;
      kind = opcode == DISK_CLOSE ? JOB_DISK_CLOSE : JOB_DISK_SYNC;
      break;
    default: return STORAGE_DISK_UNSUPPORTED;
  }
  WriteLE(result, StartJob(kind, owner, total), 4);
  *length = 4;
  return STORAGE_DISK_OK;
}

static uint16_t ChannelNameCommand(uint64_t owner, uint16_t opcode, const uint8_t *payload, size_t n,
                                   uint8_t *result, size_t *length)
{
  if (opcode == CHANNEL_NAME_GET) {
    if (n != 0U) return STORAGE_DISK_BAD_PAYLOAD;
    if (!StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
    if (name_status != STORAGE_DISK_OK) return (uint16_t)name_status;
    WriteLE(result, channel_name_length, 2); result[2] = result[3] = 0;
    memcpy(result + 4, channel_name, channel_name_length); *length = 4U + channel_name_length;
    return STORAGE_DISK_OK;
  }
  if (n < 2U || ReadLE(payload, 2) != n - 2U || !StorageCard_ValidName(payload + 2, n - 2U)) return STORAGE_DISK_BAD_PAYLOAD;
  if (!StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  if (profile_status != STORAGE_DISK_OK) return (uint16_t)profile_status;
  if (!CardIdle() || user_transfer.id != 0U || last_id == UINT32_MAX ||
      memcmp(card_profile.controller_uid, controller_uid, sizeof(controller_uid)) != 0) return STORAGE_DISK_BAD_STATE;
  pending_name_length = (uint16_t)(n - 2U); memcpy(pending_name, payload + 2, pending_name_length);
  WriteLE(result, StartJob(JOB_CHANNEL_NAME, owner, pending_name_length), 4); *length = 4;
  return STORAGE_DISK_OK;
}

static uint16_t CatalogCommand(uint64_t owner, uint16_t opcode, const uint8_t *payload, size_t n, uint8_t *result, size_t *length)
{
  if (opcode == EMULATION_INFO) {
    if (n != 0U) return STORAGE_DISK_BAD_PAYLOAD;
    WriteLE(result, 7U, 4); result[4] = atomic_load(&optical_enabled); result[5] = atomic_load(&optical_applied); result[6] = result[7] = 0;
    *length = 8; return STORAGE_DISK_OK;
  }
  if (opcode == OPTICAL_SET) {
    if (n != 1U || payload[0] > 1U) return STORAGE_DISK_BAD_PAYLOAD;
    if (active_job != NULL || raw_disk.state != RAW_CLOSED ||
        (current.state != IDLE && current.state != IMAGE_READY && current.state != NO_CARD && current.state != STORAGE_ERROR)) return STORAGE_DISK_BAD_STATE;
    atomic_store(&optical_enabled, payload[0] != 0U); return STORAGE_DISK_OK;
  }
  if (opcode == DISK_LIST_RESULT) {
    if (n != 4U) return STORAGE_DISK_BAD_PAYLOAD;
    if (catalog.owner != owner || catalog.job == 0U || catalog.job != ReadLE(payload, 4)) return STORAGE_DISK_NOT_FOUND;
    WriteLE(result, catalog.next, 4); result[4] = catalog.count; result[5] = result[6] = result[7] = 0;
    for (uint8_t i = 0; i < catalog.count; ++i) {
      uint8_t *item = result + 8U + 12U * i; WriteLE(item, catalog.items[i].id, 4); WriteLE(item + 4, catalog.items[i].sectors, 4);
      item[8] = (uint8_t)catalog.items[i].format; item[9] = item[10] = item[11] = 0;
    }
    *length = 8U + 12U * catalog.count; return STORAGE_DISK_OK;
  }
  if (opcode != DISK_LIST && opcode != DISK_DELETE) return STORAGE_DISK_UNSUPPORTED;
  if (n != 4U) return STORAGE_DISK_BAD_PAYLOAD;
  if (!StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  if (active_job != NULL || user_transfer.id != 0U || last_id == UINT32_MAX ||
      raw_disk.state == RAW_PENDING || raw_disk.state == RAW_PROVISIONING ||
      (current.state != IDLE && current.state != IMAGE_READY && current.state != SERVING && current.state != STORAGE_ERROR)) return STORAGE_DISK_BAD_STATE;
  if (opcode == DISK_DELETE && raw_disk.state != RAW_CLOSED) return STORAGE_DISK_BAD_STATE;
  const uint32_t value = (uint32_t)ReadLE(payload, 4);
  if (opcode == DISK_DELETE && (value == 0U || value > WRITABLE_DISK_MAX_ID)) return STORAGE_DISK_RANGE;
  catalog.owner = owner; catalog.job = 0; catalog.count = 0;
  if (opcode == DISK_LIST) catalog.cursor = value; else catalog.remove_id = value;
  WriteLE(result, StartJob(opcode == DISK_LIST ? JOB_DISK_LIST : JOB_DISK_DELETE, owner, 0), 4); *length = 4;
  return STORAGE_DISK_OK;
}

static uint16_t Command(uint64_t owner, uint16_t opcode, const uint8_t *payload, size_t length,
                        uint8_t *result, size_t *result_length, uint32_t *detail)
{
  if (opcode == UPLOAD_DATA) return DataCommand(owner, payload, length, result, result_length, detail);
  StorageDiskImage candidate;
  if (opcode == UPLOAD_BEGIN) {
    const StorageDiskResult decoded = StorageDisk_DecodeImage(payload, length, &candidate);
    if (decoded != STORAGE_DISK_OK) return (uint16_t)decoded;
  }
  taskENTER_CRITICAL();
  uint16_t status;
  if (opcode == CHANNEL_NAME_GET || opcode == CHANNEL_NAME_SET) {
    status = ChannelNameCommand(owner, opcode, payload, length, result, result_length);
  } else if (opcode >= EMULATION_INFO && opcode <= DISK_DELETE) {
    status = CatalogCommand(owner, opcode, payload, length, result, result_length);
  } else if (opcode == UPLOAD_BEGIN) {
    status = BeginCommand(owner, &candidate, result, result_length, detail);
  } else if (opcode >= CARD_INFO && opcode <= CARD_FORMAT) {
    status = CardCommand(owner, opcode, payload, length, result, result_length, detail);
  } else if (opcode >= DISK_CREATE && opcode <= DISK_INFO) {
    status = WritableCommand(owner, opcode, payload, length, result, result_length, detail);
  } else {
    status = OtherCommand(owner, opcode, payload, length, result, result_length);
  }
  if (status == STORAGE_DISK_BAD_STATE) *detail = current.state;
  taskEXIT_CRITICAL();
  if (status == STORAGE_DISK_OK && opcode != JOB_STATUS && opcode != IMAGE_INFO &&
      opcode != CARD_INFO && opcode != CARD_USER_READ && opcode != DISK_INFO && opcode != DISK_RESULT && opcode != EMULATION_INFO && opcode != DISK_LIST_RESULT && opcode != CHANNEL_NAME_GET) xTaskNotifyGive(worker);
  return status;
}

static void Status(ManagementStorageStatus *status)
{
  taskENTER_CRITICAL();
  *status = current;
  taskEXIT_CRITICAL();
}

static void SessionEnded(uint64_t owner)
{
  taskENTER_CRITICAL();
  const bool card_job = active_job != NULL && active_job->owner == owner &&
                        (active_job->kind == JOB_CARD_INIT || active_job->kind == JOB_USER_WRITE || active_job->kind == JOB_CHANNEL_NAME);
  const bool closing_raw = raw_disk.owner == owner && !RawExported();
  if (closing_raw) raw_disk.close_requested = true;
  const bool raw_job = closing_raw && active_job != NULL && WritableJob(active_job->kind);
  const bool cancel = upload_owner == owner || card_job || raw_job;
  if (cancel) abort_requested = true;
  const bool storing_user = card_job && active_job->kind == JOB_USER_WRITE;
  if (user_transfer.owner == owner && !storing_user) user_transfer.id = 0;
  for (unsigned i = 0; i < JOB_COUNT; ++i) {
    if (jobs[i].owner == owner) {
      jobs[i].owner = 0;
      jobs[i].event_pending = false;
    }
  }
  taskEXIT_CRITICAL();
  if (cancel || closing_raw) xTaskNotifyGive(worker);
}

static bool TakeEvent(uint64_t owner, uint8_t result[32])
{
  taskENTER_CRITICAL();
  bool found = false;
  for (unsigned i = 0; i < JOB_COUNT; ++i) {
    StorageJob *job = &jobs[(next_job_slot + i) % JOB_COUNT];
    if (job->event_pending && job->owner == owner) {
      job->event_pending = false;
      EncodeJob(job, result);
      found = true;
      break;
    }
  }
  taskEXIT_CRITICAL();
  return found;
}

void StorageService_Start(void)
{
  for (unsigned i = 0; i < STORAGE_CONTROLLER_UID_BYTES; ++i) controller_uid[i] = ((const volatile uint8_t *)UID_BASE)[i];
  worker = xTaskCreateStatic(StorageTask, "storage",
                             sizeof(task_stack) / sizeof(task_stack[0]), NULL,
                             (UBaseType_t)osPriorityNormal, task_stack, &task_control);
  if (worker == NULL) Error_Handler();
}

void StorageService_SetUid(const uint8_t uid[STORAGE_CONTROLLER_UID_BYTES])
{
  taskENTER_CRITICAL();
  memcpy(controller_uid, uid, sizeof(controller_uid));
  taskEXIT_CRITICAL();
}

const ManagementStorageBackend *StorageService_Backend(void)
{
  static const ManagementStorageBackend backend = {
    .command = Command, .status = Status, .end_session = SessionEnded, .take_event = TakeEvent,
  };
  return &backend;
}
