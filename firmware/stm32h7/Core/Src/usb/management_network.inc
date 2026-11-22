/* Ethernet wire framing runs only on the management USB task. */
enum { NET_HEADER = 16, NET_FRAGMENT = MAX_PAYLOAD - NET_HEADER, NET_TIMEOUT_MS = 1000 };

static uint8_t network_in[MANAGEMENT_ETH_MAX] MANAGEMENT_SECTION(".protocol.network");
static uint8_t network_out[MANAGEMENT_ETH_MAX] MANAGEMENT_SECTION(".protocol.network");
static bool network_enabled;
static uint32_t network_credit_drops;
static uint32_t network_last_in_id, network_in_crc;
static uint16_t network_in_length, network_in_offset;
static uint64_t network_in_deadline;
static uint32_t network_out_id, network_out_crc, network_sequence;
static uint16_t network_out_length, network_out_offset;

static void NetworkDisable(void)
{
  if (network_enabled) (void)board.network->set_enabled(false);
  network_enabled = false;
  network_in_length = 0;
  network_out_length = 0;
}

static void NetworkReset(void)
{
  NetworkDisable();
  network_last_in_id = 0;
  network_out_id = 0;
  network_sequence = 0;
  previous_network = false;
  prefer_network = false;
}

static void NetworkTick(void)
{
  if (network_enabled && (!board.network->configured() || StorageExclusive(GetStorage().state))) {
    NetworkDisable();
  }
  if (network_in_length != 0U && now >= network_in_deadline) network_in_length = 0;
}

static bool NetworkUp(void)
{
  return network_enabled && board.network->link_up();
}

static uint16_t NetworkInput(const uint8_t *payload, size_t bytes)
{
  if (!NetworkUp()) return NOT_READY;
  if (bytes <= NET_HEADER || bytes > MAX_PAYLOAD || ReadLE(payload + 12, 4) != 0U) return BAD_PAYLOAD;
  const uint32_t id = (uint32_t)ReadLE(payload, 4);
  const uint16_t offset = (uint16_t)ReadLE(payload + 4, 2);
  const uint16_t total = (uint16_t)ReadLE(payload + 6, 2);
  const uint32_t crc = (uint32_t)ReadLE(payload + 8, 4);
  if (total < MANAGEMENT_ETH_MIN || total > MANAGEMENT_ETH_MAX || offset >= total) return BAD_PAYLOAD;
  const size_t fragment = bytes - NET_HEADER;
  const size_t remaining = (size_t)(total - offset);
  if (fragment != (remaining < NET_FRAGMENT ? remaining : NET_FRAGMENT)) return BAD_PAYLOAD;
  if (offset == 0U) {
    if (id <= network_last_in_id || network_in_length != 0U) return BAD_STATE;
    if (!board.network->tx_available()) return BUSY;
    network_last_in_id = id;
    network_in_length = total;
    network_in_offset = 0;
    network_in_crc = crc;
    network_in_deadline = now + NET_TIMEOUT_MS;
  }
  if (id != network_last_in_id || network_in_length == 0U) return BAD_STATE;
  if (offset != network_in_offset || total != network_in_length || crc != network_in_crc) {
    network_in_length = 0;
    return BAD_PAYLOAD;
  }
  memcpy(network_in + offset, payload + NET_HEADER, fragment);
  network_in_offset = (uint16_t)(offset + fragment);
  if (network_in_offset < total) return OK;
  network_in_length = 0;
  if (Crc32(network_in, total) != crc) return CHECKSUM_MISMATCH;
  return board.network->submit(network_in, total) ? OK : NOT_READY;
}

static uint16_t HandleNetwork(uint16_t op, const uint8_t *payload, size_t bytes,
                              uint8_t *result, size_t *length)
{
  if (board.network == NULL) return UNSUPPORTED;
  switch (op) {
    case NET_SET:
      if (bytes != 1U || payload[0] > 1U) return BAD_PAYLOAD;
      if (payload[0] == 0U) {
        NetworkDisable();
        return OK;
      }
      if (StorageExclusive(GetStorage().state)) return BAD_STATE;
      if (!board.network->set_enabled(true)) return NOT_READY;
      network_enabled = true;
      return OK;
    case NET_FRAME:
      return NetworkInput(payload, bytes);
    case NET_STATS:
      if (bytes != 0U) return BAD_PAYLOAD;
      WriteLE(result, board.network->rx_dropped(), 4);
      WriteLE(result + 4, board.network->tx_dropped(), 4);
      WriteLE(result + 8, network_credit_drops, 4);
      result[12] = network_enabled;
      result[13] = NetworkUp();
      result[14] = 0;
      result[15] = 0;
      *length = 16;
      return OK;
    default:
      return UNSUPPORTED;
  }
}

static bool PollNetwork(void)
{
  if (!NetworkUp()) return false;
  if (network_out_length == 0U) {
    const uint16_t length = board.network->take_rx(network_out);
    if (length == 0U) return false;
    const unsigned fragments = (length + NET_FRAGMENT - 1U) / NET_FRAGMENT;
    const unsigned cost = length + NET_HEADER * fragments;
    if (credit[CHANNEL_NETWORK] - sent[CHANNEL_NETWORK] < cost) {
      ++network_credit_drops;
      return false;
    }
    if (network_out_id == UINT32_MAX || UINT32_MAX - network_sequence < fragments) {
      NetworkDisable();
      return false;
    }
    ++network_out_id;
    network_out_length = length;
    network_out_offset = 0;
    network_out_crc = Crc32(network_out, length);
  }
  const uint16_t remaining = (uint16_t)(network_out_length - network_out_offset);
  const uint16_t size = remaining < NET_FRAGMENT ? remaining : NET_FRAGMENT;
  uint8_t payload[MAX_PAYLOAD] = {0};
  WriteLE(payload, network_out_id, 4);
  WriteLE(payload + 4, network_out_offset, 2);
  WriteLE(payload + 6, network_out_length, 2);
  WriteLE(payload + 8, network_out_crc, 4);
  memcpy(payload + NET_HEADER, network_out + network_out_offset, size);
  Send(STREAM, CHANNEL_NETWORK, NET_RX, session, ++network_sequence, payload, NET_HEADER + size);
  sent[CHANNEL_NETWORK] += NET_HEADER + size;
  network_out_offset += size;
  if (network_out_offset == network_out_length) network_out_length = 0;
  return true;
}
