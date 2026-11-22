#include "firmware_version.h"
#include "management_protocol.h"
#include "target_hid_report.h"
#include <string.h>

#if defined(STM32H743xx)
#define MANAGEMENT_SECTION(name) __attribute__((section(name)))
#else
#define MANAGEMENT_SECTION(name)
#endif


enum {
  HEADER_SIZE = 24, MAX_PAYLOAD = 1024, CRC_SIZE = 4,
  MAX_RAW = HEADER_SIZE + MAX_PAYLOAD + CRC_SIZE, MAX_ENCODED = 1057,
  MAX_RESULT = 256, RESPONSE_PREFIX = 8, LEASE_MS = 2000, LOG_COUNT = 128,
  SYSTEM = 0, CHANNEL_HID = 1, CHANNEL_SERIAL = 2, CHANNEL_STORAGE = 3, CHANNEL_NETWORK = 4, CHANNEL_DEBUG = 5, REQUEST = 1, RESPONSE = 2, EVENT = 3, STREAM = 4,
  OK = 0, UNSUPPORTED = 1, BAD_PAYLOAD = 2, BAD_STATE = 3, BUSY = 4, NO_SESSION = 5,
  STALE_SEQUENCE = 6, ID_CONFLICT = 7, SEQUENCE_GAP = 8, NOT_READY = 9, RANGE = 10, CHECKSUM_MISMATCH = 12,
  HELLO = 1, PING = 2, INFO = 3, STATUS = 4, ECHO = 5, RX_LIMIT = 6, JOB_STATUS = 7, JOB_CANCEL = 8, UPLOAD_CANCEL = 9,
  LOG_CONFIG = 0x0501, LOG_STATS = 0x0502, LOG_RECORD = 0x0580,
  HID_DONE = 0x0180, STATE_CHANGED = 0x0080, JOB_CHANGED = 0x0081, UPLOAD_BEGIN = 0x0301, IMAGE_MOUNT = 0x0306, IMAGE_UNMOUNT = 0x0307,
  SERIAL_WRITE = 0x0201, SERIAL_STATUS = 0x0202, SERIAL_STATS = 0x0203, SERIAL_RX = 0x0280,
  NET_SET = 0x0401, NET_FRAME = 0x0402, NET_STATS = 0x0403, NET_RX = 0x0480,
  FEATURE_HID = 1, FEATURE_SERIAL = 1 << 1, FEATURE_STORAGE = 1 << 2, FEATURE_OPTICAL = 1 << 4, FEATURE_NETWORK = 1 << 5, FEATURE_DEBUG = 1 << 6,
};

typedef struct {
  uint16_t request_length;
  uint16_t response_length;
  uint8_t request[HEADER_SIZE + MAX_PAYLOAD];
  uint8_t response[RESPONSE_PREFIX + MAX_RESULT];
} Replay;

typedef struct {
  uint8_t bytes[256];
} LogRecord;

static Replay replay[MANAGEMENT_CHANNELS][4] MANAGEMENT_SECTION(".protocol.replay");
static LogRecord logs[LOG_COUNT] MANAGEMENT_SECTION(".debug_log");
static uint8_t rx_encoded[MAX_ENCODED] MANAGEMENT_SECTION(".protocol.rx");
static uint8_t rx_raw[MAX_RAW] MANAGEMENT_SECTION(".protocol.rx");
static uint8_t tx_raw[MAX_RAW]
    MANAGEMENT_SECTION(".protocol.tx");
static uint8_t tx_encoded[sizeof(tx_raw) + sizeof(tx_raw) / 254U + 2U]
    MANAGEMENT_SECTION(".protocol.tx");

static ManagementPlatform board;
static uint16_t rx_length, tx_length, tx_offset;
static bool discard_input;
static uint32_t crc_errors, framing_errors;
static uint64_t now, rx_last_byte, lease_at, session, nonce;
static uint32_t expected[MANAGEMENT_CHANNELS];
static uint64_t credit[MANAGEMENT_CHANNELS], sent[MANAGEMENT_CHANNELS];
static uint32_t log_produced, log_sent, log_dropped, log_wire_sequence;
static uint32_t hid_event_sequence, serial_sequence, state_sequence;
static ManagementSerialStatus previous_serial;
static bool previous_network, prefer_network;
static ManagementStorageStatus previous_storage;
static bool serial_enabled;
static uint8_t log_head, log_size, log_level;
static bool log_enabled;
static uint16_t log_rate;
static uint32_t log_modules;
static uint64_t log_next_send;

static uint64_t ReadLE(const uint8_t *bytes, size_t n)
{
  uint64_t value = 0;
  for (size_t i = 0; i < n; ++i) {
    value |= (uint64_t)bytes[i] << (8U * i);
  }
  return value;
}

static void WriteLE(uint8_t *bytes, uint64_t value, size_t n)
{
  for (size_t i = 0; i < n; ++i) {
    bytes[i] = (uint8_t)value;
    value >>= 8U;
  }
}

static uint32_t Crc32(const uint8_t *bytes, size_t n)
{
  uint32_t crc = UINT32_MAX;
  for (size_t i = 0; i < n; ++i) {
    crc ^= bytes[i];
    for (uint8_t bit = 0; bit < 8; ++bit) {
      crc = (crc >> 1) ^ (0xEDB88320U & (0U - (crc & 1U)));
    }
  }
  return ~crc;
}

static size_t Decode(void)
{
  size_t in = 0, out = 0;
  while (in < rx_length) {
    const unsigned code = rx_encoded[in++];
    if (code == 0U || code - 1U > rx_length - in) {
      return 0;
    }
    const size_t block = code - 1U;
    const bool zero = (code != 255U && in + block < rx_length);
    if (out + block + zero > sizeof(rx_raw)) {
      return 0;
    }
    memcpy(&rx_raw[out], &rx_encoded[in], block);
    in += block;
    out += block;
    if (zero) {
      rx_raw[out++] = 0;
    }
  }
  return out;
}

static void Send(uint8_t kind, uint8_t channel, uint16_t op, uint64_t id,
                 uint32_t sequence, const uint8_t *payload, size_t length)
{
  memset(tx_raw, 0, HEADER_SIZE);
  WriteLE(tx_raw, 0x4D4B, 2);
  tx_raw[2] = 1;
  tx_raw[3] = kind;
  tx_raw[5] = channel;
  WriteLE(tx_raw + 6, op, 2);
  WriteLE(tx_raw + 8, id, 8);
  WriteLE(tx_raw + 16, sequence, 4);
  WriteLE(tx_raw + 20, length, 2);
  memcpy(tx_raw + HEADER_SIZE, payload, length);
  const size_t body = HEADER_SIZE + length;
  WriteLE(tx_raw + body, Crc32(tx_raw, body), CRC_SIZE);

  size_t out = 1, code_index = 0;
  uint8_t code = 1;
  for (size_t i = 0; i < body + CRC_SIZE; ++i) {
    if (tx_raw[i] != 0U) {
      tx_encoded[out++] = tx_raw[i];
      if (++code != 255U) {
        continue;
      }
    }
    tx_encoded[code_index] = code;
    code_index = out++;
    code = 1;
  }
  tx_encoded[code_index] = code;
  tx_encoded[out++] = 0;
  tx_length = (uint16_t)out;
  tx_offset = 0;
}

static void ReplyError(uint8_t channel, uint16_t op, uint64_t id, uint32_t seq,
                       uint16_t status, uint32_t detail)
{
  uint8_t payload[RESPONSE_PREFIX] = {0};
  WriteLE(payload, status, 2);
  WriteLE(payload + 4, detail, 4);
  Send(RESPONSE, channel, op, id, seq, payload, sizeof(payload));
}

static ManagementStorageStatus GetStorage(void)
{
  ManagementStorageStatus storage = {0};
  if (board.storage != NULL) {
    board.storage->status(&storage);
  } else if (board.storage_state != NULL) {
    storage.state = board.storage_state();
  }
  return storage;
}

static bool StorageExclusive(uint8_t state)
{
  return (state >= 2U && state <= 4U) || state == MANAGEMENT_STORAGE_FORMATTING;
}

#include "management_network.inc"

static void RefreshSerialSession(void)
{
  if (board.serial == NULL) return;
  const bool enable = session != 0U && !StorageExclusive(GetStorage().state);
  if (serial_enabled != enable) {
    serial_enabled = enable;
    board.serial->set_session(enable);
  }
}

static void EndSession(void)
{
  const uint64_t old_session = session;
  session = 0;
  nonce = 0;
  hid_event_sequence = serial_sequence = state_sequence = 0;
  memset(&previous_serial, 0, sizeof(previous_serial));
  memset(&previous_storage, 0, sizeof(previous_storage));
  if (old_session != 0U && board.storage != NULL) board.storage->end_session(old_session);
  RefreshSerialSession();
  NetworkReset();
  if (old_session != 0U && board.hid != NULL) {
    board.hid->end_session(board.hid->context, old_session);
  }
  log_enabled = false;
  log_level = 2;
  log_modules = 0x1FF;
  log_rate = 20;
  log_wire_sequence = 0;
  log_next_send = 0;
  memset(credit, 0, sizeof(credit));
  memset(sent, 0, sizeof(sent));
  for (unsigned channel = 0; channel < MANAGEMENT_CHANNELS; ++channel) {
    expected[channel] = 1;
    for (unsigned slot = 0; slot < 4; ++slot) {
      replay[channel][slot].request_length = 0;
    }
  }
}

void Management_Reset(void)
{
  EndSession();
  rx_length = 0;
  tx_length = 0;
  tx_offset = 0;
  discard_input = false;
}

void Management_Init(const ManagementPlatform *platform, uint64_t now_ms)
{
  board = *platform;
  now = now_ms;
  crc_errors = framing_errors = 0;
  log_head = log_size = 0;
  log_produced = log_sent = log_dropped = 0;
  serial_enabled = false;
  network_credit_drops = 0;
  Management_Reset();
}

void Management_Tick(uint64_t now_ms)
{
  now = now_ms;
  if (session != 0 && now - lease_at >= LEASE_MS) {
    EndSession();
  }
  if (rx_length != 0 && now - rx_last_byte >= LEASE_MS) {
    rx_length = 0;
    discard_input = true;
    ++framing_errors;
  }
  RefreshSerialSession();
  NetworkTick();
}

static uint32_t Features(void)
{
  return FEATURE_DEBUG | (board.hid == NULL ? 0U : FEATURE_HID) |
         (board.serial == NULL ? 0U : FEATURE_SERIAL) |
         (board.storage == NULL ? 0U : FEATURE_STORAGE) |
         (board.network == NULL ? 0U : FEATURE_NETWORK) |
         (board.optical ? FEATURE_OPTICAL : 0U);
}

static void Info(uint8_t *result)
{
  memset(result, 0, 40);
  WriteLE(result, nonce, 8);
  memcpy(result + 8, board.uid, MANAGEMENT_UID_BYTES);
  WriteLE(result + 20, MINIKMB_FW_MAJOR, 2);
  WriteLE(result + 22, MINIKMB_FW_MINOR, 2);
  WriteLE(result + 24, MINIKMB_FW_PATCH, 2);
  WriteLE(result + 26, MAX_PAYLOAD, 2);
  WriteLE(result + 28, Features(), 4);
  WriteLE(result + 32, LEASE_MS, 4);
  result[36] = 4;
}

static void Hello(const uint8_t *data, uint16_t length)
{
  if (length != 12U || ReadLE(data + 2, 2) != 0U || ReadLE(data + 4, 8) == 0U) {
    ReplyError(SYSTEM, HELLO, 0, 1, BAD_PAYLOAD, 0);
    return;
  }
  if (data[0] != 1U || data[1] < 1U) {
    ReplyError(SYSTEM, HELLO, 0, 1, UNSUPPORTED, 1);
    return;
  }
  const uint64_t requested_nonce = ReadLE(data + 4, 8);
  if (session == 0 || nonce != requested_nonce) {
    EndSession();
    session = board.new_session();
    if (session == 0) {
      ReplyError(SYSTEM, HELLO, 0, 1, NOT_READY, 0);
      return;
    }
    nonce = requested_nonce;
    RefreshSerialSession();
    Management_Log(2, 3, "Management session started");
  }
  lease_at = now;
  uint8_t response[RESPONSE_PREFIX + 40] = {0};
  Info(response + RESPONSE_PREFIX);
  Send(RESPONSE, SYSTEM, HELLO, session, 1, response, sizeof(response));
}

static void Status(uint8_t *result, bool cdc_open)
{
  memset(result, 0, 64);
  WriteLE(result, now, 8);
  WriteLE(result + 8, Features(), 4);
  result[12] = board.target_state();
  const ManagementStorageStatus storage = GetStorage();
  result[13] = storage.state;
  result[14] = cdc_open;
  result[15] = NetworkUp();
  WriteLE(result + 16, storage.job_id, 4);
  WriteLE(result + 20, storage.image_id, 4);
  WriteLE(result + 24, storage.upload_id, 4);
  WriteLE(result + 28, log_dropped, 4);
  WriteLE(result + 32, crc_errors, 4);
  WriteLE(result + 36, framing_errors, 4);
  WriteLE(result + 40, storage.received, 8);
  WriteLE(result + 48, storage.total, 8);
  WriteLE(result + 56, storage.last_error, 2);
  const uint32_t dropped = board.hid == NULL ? 0U : board.hid->dropped_events(board.hid->context);
  WriteLE(result + 60, dropped + storage.dropped_events, 4);
}

static uint16_t HandleSystem(uint16_t op, const uint8_t *p, uint16_t n,
                             uint8_t *result, size_t *result_length, uint32_t *detail)
{
  switch (op) {
    case PING:
      if (n != 8) return BAD_PAYLOAD;
      memcpy(result, p, 8);
      WriteLE(result + 8, now, 8);
      *result_length = 16;
      return OK;
    case INFO:
      if (n != 0) return BAD_PAYLOAD;
      Info(result);
      *result_length = 40;
      return OK;
    case STATUS: {
      if (n != 0) return BAD_PAYLOAD;
      ManagementSerialStatus serial = {0};
      if (board.serial != NULL) board.serial->status(&serial);
      Status(result, serial.open);
      *result_length = 64;
      return OK;
    }
    case ECHO:
      if (n > MAX_RESULT) return BAD_PAYLOAD;
      memcpy(result, p, n);
      *result_length = n;
      return OK;
    case RX_LIMIT: {
      if (n != 12 || (p[0] != 2 && p[0] != 4 && p[0] != CHANNEL_DEBUG) ||
          ReadLE(p + 1, 3) != 0) return BAD_PAYLOAD;
      const uint8_t channel = p[0];
      const uint64_t limit = ReadLE(p + 4, 8);
      if (limit > credit[channel]) {
        if (limit - sent[channel] > 32768U) return RANGE;
        credit[channel] = limit;
      }
      WriteLE(result, sent[channel], 8);
      WriteLE(result + 8, credit[channel], 8);
      *result_length = 16;
      return OK;
    }
    case JOB_STATUS:
    case JOB_CANCEL:
    case UPLOAD_CANCEL:
      if (board.storage != NULL) {
        return board.storage->command(session, op, p, n, result, result_length, detail);
      }
      return UNSUPPORTED;
    default:
      return UNSUPPORTED;
  }
}

static uint16_t HandleDebug(uint16_t op, const uint8_t *p, uint16_t n,
                            uint8_t *result, size_t *result_length)
{
  if (op == LOG_CONFIG) {
    if (n != 12 || p[0] > 1 || p[1] > 5 || ReadLE(p + 2, 2) != 0 ||
        ReadLE(p + 4, 4) > 0x1FF || ReadLE(p + 10, 2) != 0) {
      return BAD_PAYLOAD;
    }
    const uint16_t rate = (uint16_t)ReadLE(p + 8, 2);
    if (rate < 1 || rate > 100) return RANGE;
    log_enabled = p[0] != 0;
    log_level = p[1];
    log_modules = (uint32_t)ReadLE(p + 4, 4);
    log_rate = rate;
    memcpy(result, p, 12);
    *result_length = 12;
    return OK;
  }
  if (op == LOG_STATS) {
    if (n != 0) return BAD_PAYLOAD;
    WriteLE(result, log_produced, 4);
    WriteLE(result + 4, log_sent, 4);
    WriteLE(result + 8, log_dropped, 4);
    WriteLE(result + 12, log_size, 4);
    *result_length = 16;
    return OK;
  }
  return UNSUPPORTED;
}

static uint16_t HandleHid(uint16_t op, const uint8_t *payload, size_t length,
                          uint8_t *result, size_t *result_length, uint32_t *detail)
{
  if (board.hid == NULL || op < 0x0101U || op > TARGET_HID_RELEASE_ALL) {
    return UNSUPPORTED;
  }
  const uint8_t storage = GetStorage().state;
  if (op != TARGET_HID_RELEASE_ALL && StorageExclusive(storage)) {
    *detail = storage;
    return BAD_STATE;
  }

  uint32_t ticket = 0U;
  const uint16_t status = board.hid->submit(board.hid->context, session, op, payload,
                                            length, &ticket);
  if (status == OK) {
    WriteLE(result, ticket, 4);
    *result_length = 4;
  } else if (status == BUSY) {
    *detail = 10U;
  } else if (status == NOT_READY) {
    *detail = 1U;
  }
  return status;
}

static uint16_t HandleSerial(uint16_t op, const uint8_t *data, uint16_t length,
                             uint8_t *result, size_t *result_length, uint32_t *detail)
{
  if (board.serial == NULL) return UNSUPPORTED;
  if (op == SERIAL_WRITE) {
    if (length == 0U || length > MAX_RESULT) return BAD_PAYLOAD;
    const uint8_t storage = GetStorage().state;
    if (StorageExclusive(storage)) {
      *detail = storage;
      return BAD_STATE;
    }
    const uint8_t target = board.target_state();
    if (target != 1U && target != 2U) {
      *detail = 1U;
      return NOT_READY;
    }
    const uint16_t status = board.serial->submit(data, length);
    if (status == OK) {
      WriteLE(result, length, 2);
      *result_length = 2;
    } else if (status == BUSY) {
      *detail = 10U;
    } else if (status == NOT_READY) {
      *detail = 3U;
    }
    return status;
  }
  if (op != SERIAL_STATUS && op != SERIAL_STATS) return UNSUPPORTED;
  if (length != 0U) return BAD_PAYLOAD;
  ManagementSerialStatus state;
  board.serial->status(&state);
  if (op == SERIAL_STATS) {
    WriteLE(result, state.tx_discarded, 4);
    WriteLE(result + 4, state.rx_discarded, 4);
    *result_length = 8U;
  } else {
    result[0] = state.open;
    result[1] = state.stop_bits;
    result[2] = state.parity;
    result[3] = state.data_bits;
    WriteLE(result + 4, state.baud, 4);
    WriteLE(result + 8, state.tx_queued, 4);
    WriteLE(result + 12, state.rx_queued, 4);
    *result_length = 16U;
  }
  return OK;
}

static void Dispatch(size_t raw_length)
{
  if (raw_length < HEADER_SIZE + CRC_SIZE ||
      ReadLE(rx_raw, 2) != 0x4D4B) {
    ++framing_errors;
    return;
  }
  const uint16_t length = (uint16_t)ReadLE(rx_raw + 20, 2);
  const size_t body = HEADER_SIZE + length;
  if (length > MAX_PAYLOAD || raw_length != body + CRC_SIZE) {
    ++framing_errors;
    return;
  }
  if (Crc32(rx_raw, body) != ReadLE(rx_raw + body, CRC_SIZE)) {
    ++crc_errors;
    return;
  }
  if (rx_raw[3] != REQUEST) return;

  const uint8_t channel = rx_raw[5];
  const uint16_t op = (uint16_t)ReadLE(rx_raw + 6, 2);
  const uint64_t id = ReadLE(rx_raw + 8, 8);
  const uint32_t seq = (uint32_t)ReadLE(rx_raw + 16, 4);
  const uint8_t *payload = rx_raw + HEADER_SIZE;
  if (rx_raw[2] != 1) {
    ReplyError(channel, op, id, seq, UNSUPPORTED, rx_raw[2]);
    return;
  }
  if (channel >= MANAGEMENT_CHANNELS || seq == 0 || seq == UINT32_MAX ||
      rx_raw[4] != 0 || ReadLE(rx_raw + 22, 2) != 0) {
    ReplyError(channel, op, id, seq, BAD_PAYLOAD, 0);
    return;
  }
  if (channel == SYSTEM && op == HELLO) {
    if (id == 0 && seq == 1) {
      Hello(payload, length);
    } else {
      ReplyError(channel, op, id, seq, BAD_PAYLOAD, 0);
    }
    return;
  }
  if (session == 0 || session != id) {
    ReplyError(channel, op, id, seq, NO_SESSION, 0);
    return;
  }

  const uint32_t next = expected[channel];
  Replay *entry = &replay[channel][(seq - 1U) % 4U];
  if (seq < next) {
    if (next - seq > 4U || entry->request_length == 0) {
      ReplyError(channel, op, id, seq, STALE_SEQUENCE, next);
    } else if (entry->request_length != body || memcmp(entry->request, rx_raw, body) != 0) {
      ReplyError(channel, op, id, seq, ID_CONFLICT, 0);
    } else {
      lease_at = now;
      Send(RESPONSE, channel, op, id, seq, entry->response, entry->response_length);
    }
    return;
  }
  if (seq > next) {
    ReplyError(channel, op, id, seq, SEQUENCE_GAP, next);
    return;
  }

  expected[channel] = next + 1U;
  lease_at = now;
  entry->request_length = (uint16_t)body;
  memcpy(entry->request, rx_raw, body);
  uint8_t *result = entry->response + RESPONSE_PREFIX;
  size_t result_length = 0;
  uint32_t detail = 0;
  uint16_t status = UNSUPPORTED;
  if (channel == SYSTEM) {
    status = HandleSystem(op, payload, length, result, &result_length, &detail);
  } else if (channel == CHANNEL_DEBUG) {
    status = HandleDebug(op, payload, length, result, &result_length);
  } else if (channel == CHANNEL_HID) {
    status = HandleHid(op, payload, length, result, &result_length, &detail);
  } else if (channel == CHANNEL_SERIAL) {
    status = HandleSerial(op, payload, length, result, &result_length, &detail);
  } else if (channel == CHANNEL_NETWORK) {
    status = HandleNetwork(op, payload, length, result, &result_length);
  } else if (channel == CHANNEL_STORAGE && op >= UPLOAD_BEGIN && board.storage != NULL) {
    if (board.optical || (op != IMAGE_MOUNT && op != IMAGE_UNMOUNT)) {
      status = board.storage->command(session, op, payload, length, result, &result_length, &detail);
    }
    if ((op == UPLOAD_BEGIN || op == MANAGEMENT_CARD_FORMAT) && status == OK) {
      if (board.hid != NULL) board.hid->end_session(board.hid->context, session);
      RefreshSerialSession();
      NetworkDisable();
    }
  }
  memset(entry->response, 0, RESPONSE_PREFIX);
  WriteLE(entry->response, status, 2);
  WriteLE(entry->response + 4, status == UNSUPPORTED ? op : detail, 4);
  entry->response_length = (uint16_t)(RESPONSE_PREFIX + result_length);
  Send(RESPONSE, channel, op, id, seq, entry->response, entry->response_length);
}

void Management_Input(uint8_t byte)
{
  if (byte == 0U) {
    if (!discard_input && rx_length != 0) Dispatch(Decode());
    rx_length = 0;
    discard_input = false;
  } else if (!discard_input) {
    if (rx_length == sizeof(rx_encoded)) {
      discard_input = true;
      rx_length = 0;
      ++framing_errors;
    } else {
      rx_encoded[rx_length++] = byte;
      rx_last_byte = now;
    }
  }
}

const uint8_t *Management_Output(size_t *length)
{
  *length = tx_length - tx_offset;
  return tx_encoded + tx_offset;
}

void Management_OutputConsumed(size_t length)
{
  if (length <= (size_t)(tx_length - tx_offset)) {
    tx_offset += (uint16_t)length;
  }
}

void Management_Log(uint8_t level, uint8_t module, const char *message)
{
  if (module > 8 || level > 5) return;
  ++log_produced;
  if (log_size == LOG_COUNT) {
    if (session != 0 && log_enabled) {
      ++log_dropped;
      return;
    }
    log_head = (uint8_t)((log_head + 1U) % LOG_COUNT);
    --log_size;
    ++log_dropped;
  }
  const uint8_t tail = (uint8_t)((log_head + log_size) % LOG_COUNT);
  uint8_t *record = logs[tail].bytes;
  size_t length = 0;
  while (length < 240 && message[length] != '\0') ++length;
  if (length == 240) {
    while (((uint8_t)message[length] & 0xC0U) == 0x80U) --length;
  }
  WriteLE(record, now, 8);
  WriteLE(record + 8, log_produced, 4);
  record[12] = level;
  record[13] = module;
  WriteLE(record + 14, length, 2);
  memcpy(record + 16, message, length);
  ++log_size;
}

static void PollLog(void)
{
  if (session == 0 || !log_enabled || tx_offset != tx_length || now < log_next_send) return;
  while (log_size != 0) {
    const uint8_t *record = logs[log_head].bytes;
    const size_t length = 16U + (size_t)ReadLE(record + 14, 2);
    const bool selected = record[12] >= log_level && (log_modules & (1U << record[13])) != 0;
    if (selected && credit[CHANNEL_DEBUG] - sent[CHANNEL_DEBUG] < length) return;
    if (selected && log_wire_sequence == UINT32_MAX) {
      EndSession();
      return;
    }
    log_head = (uint8_t)((log_head + 1U) % LOG_COUNT);
    --log_size;
    if (selected) {
      Send(STREAM, CHANNEL_DEBUG, LOG_RECORD, session, ++log_wire_sequence, record, length);
      sent[CHANNEL_DEBUG] += length;
      ++log_sent;
      log_next_send = now + (999U + log_rate) / log_rate;
      return;
    }
  }
}

static bool PollHid(void)
{
  ManagementHidEvent event;
  if (board.hid == NULL || !board.hid->take_event(board.hid->context, &event) ||
      event.session != session) {
    return false;
  }
  if (hid_event_sequence == UINT32_MAX) {
    EndSession();
    return true;
  }

  uint8_t payload[8] = {0};
  WriteLE(payload, event.ticket, 4);
  WriteLE(payload + 4, event.status, 2);
  Send(EVENT, CHANNEL_HID, HID_DONE, session, ++hid_event_sequence, payload, sizeof(payload));
  return true;
}

static bool PollStorageJob(void)
{
  uint8_t payload[32];
  if (board.storage == NULL || !board.storage->take_event(session, payload)) return false;
  if (state_sequence == UINT32_MAX) {
    EndSession();
  } else {
    Send(EVENT, SYSTEM, JOB_CHANGED, session, ++state_sequence, payload, sizeof(payload));
  }
  return true;
}

static bool PollState(void)
{
  if (board.serial == NULL && board.storage == NULL && board.network == NULL) return false;
  ManagementSerialStatus current = {0};
  if (board.serial != NULL) board.serial->status(&current);
  const ManagementStorageStatus storage = GetStorage();
  const bool net_up = NetworkUp();
  if (net_up == previous_network && current.open == previous_serial.open &&
      current.tx_discarded == previous_serial.tx_discarded &&
      current.rx_discarded == previous_serial.rx_discarded &&
      storage.state == previous_storage.state && storage.job_id == previous_storage.job_id &&
      storage.image_id == previous_storage.image_id &&
      storage.last_error == previous_storage.last_error) return false;
  if (state_sequence == UINT32_MAX) {
    EndSession();
  } else {
    uint8_t payload[64];
    previous_network = net_up;
    previous_serial = current;
    previous_storage = storage;
    Status(payload, current.open);
    Send(EVENT, SYSTEM, STATE_CHANGED, session, ++state_sequence, payload, sizeof(payload));
  }
  return true;
}

static bool PollSerialRx(void)
{
  const uint64_t available = credit[CHANNEL_SERIAL] - sent[CHANNEL_SERIAL];
  if (board.serial == NULL || available == 0U) return false;
  const uint8_t storage = GetStorage().state;
  if (StorageExclusive(storage)) return false;
  if (serial_sequence == UINT32_MAX) {
    EndSession();
    return true;
  }
  uint8_t bytes[MAX_RESULT];
  const uint16_t capacity = available < sizeof(bytes) ? (uint16_t)available : sizeof(bytes);
  const uint16_t length = board.serial->take_rx(bytes, capacity);
  if (length == 0U) return false;
  Send(STREAM, CHANNEL_SERIAL, SERIAL_RX, session, ++serial_sequence, bytes, length);
  sent[CHANNEL_SERIAL] += length;
  return true;
}

void Management_Poll(void)
{
  if (session == 0U || tx_offset != tx_length) {
    return;
  }
  if (PollHid() || PollStorageJob() || PollState()) return;
  if (now - log_next_send >= 100U && now >= log_next_send) {
    PollLog();
    if (tx_offset != tx_length || session == 0U) return;
  }
  if (prefer_network) {
    if (PollNetwork()) { prefer_network = false; return; }
    if (PollSerialRx()) return;
  } else {
    if (PollSerialRx()) { prefer_network = true; return; }
    if (PollNetwork()) return;
  }
  PollLog();
}
