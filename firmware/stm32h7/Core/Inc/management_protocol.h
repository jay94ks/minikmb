#ifndef MINIKMB_MANAGEMENT_PROTOCOL_H
#define MINIKMB_MANAGEMENT_PROTOCOL_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum { MANAGEMENT_UID_BYTES = 12, MANAGEMENT_CHANNELS = 6 };

typedef struct {
  uint64_t session;
  uint32_t ticket;
  uint16_t status;
} ManagementHidEvent;

typedef struct {
  void *context;
  uint16_t (*submit)(void *context, uint64_t session, uint16_t opcode,
                     const uint8_t *payload, size_t length, uint32_t *ticket);
  void (*end_session)(void *context, uint64_t session);
  bool (*take_event)(void *context, ManagementHidEvent *event);
  uint32_t (*dropped_events)(void *context);
} ManagementHidBackend;



typedef struct {
  uint32_t baud, tx_queued, rx_queued, tx_discarded, rx_discarded;
  uint8_t open, stop_bits, parity, data_bits;
} ManagementSerialStatus;

typedef struct {
  void (*set_session)(bool active);
  uint16_t (*submit)(const uint8_t *data, uint16_t length);
  uint16_t (*take_rx)(uint8_t *data, uint16_t capacity);
  void (*status)(ManagementSerialStatus *status);
} ManagementSerialBackend;

enum { MANAGEMENT_ETH_MIN = 14, MANAGEMENT_ETH_MAX = 1518 };

typedef struct {
  bool (*set_enabled)(bool enable);
  bool (*configured)(void);
  bool (*link_up)(void);
  bool (*tx_available)(void);
  bool (*submit)(const uint8_t *frame, uint16_t bytes);
  uint16_t (*take_rx)(uint8_t frame[MANAGEMENT_ETH_MAX]);
  uint32_t (*rx_dropped)(void);
  uint32_t (*tx_dropped)(void);
} ManagementNetworkBackend;

enum { MANAGEMENT_STORAGE_FORMATTING = 10, MANAGEMENT_CARD_FORMAT = 0x030F };

typedef struct {
  uint64_t received, total;
  uint32_t job_id, image_id, upload_id, dropped_events;
  uint16_t last_error;
  uint8_t state;
} ManagementStorageStatus;

typedef struct {
  uint16_t (*command)(uint64_t session, uint16_t opcode, const uint8_t *payload, size_t length,
                      uint8_t *result, size_t *result_length, uint32_t *detail);
  void (*status)(ManagementStorageStatus *status);
  void (*end_session)(uint64_t session);
  bool (*take_event)(uint64_t session, uint8_t result[32]);
} ManagementStorageBackend;

typedef struct {
  uint64_t (*new_session)(void);
  uint8_t (*storage_state)(void);
  uint8_t (*target_state)(void);
  uint8_t uid[MANAGEMENT_UID_BYTES];
  const ManagementHidBackend *hid; /* Optional; backend must outlive this instance. */
  const ManagementSerialBackend *serial;
  const ManagementStorageBackend *storage;
  const ManagementNetworkBackend *network;
  bool optical;
} ManagementPlatform;

/* Single owner task. Consume and drain the current reply before feeding more input. */
void Management_Init(const ManagementPlatform *platform, uint64_t now_ms);
void Management_Reset(void);
void Management_Tick(uint64_t now_ms);
void Management_Input(uint8_t byte);
const uint8_t *Management_Output(size_t *length);
void Management_OutputConsumed(size_t length);
void Management_Poll(void);
void Management_Log(uint8_t level, uint8_t module, const char *message);

#endif
