"""Exchange Ethernet frames with MiniKMB's NCM bridge over the management CDC."""
from collections import deque
import secrets
import struct
import time
import zlib

from probe_management import PING, RX_LIMIT

CHANNEL = 4
NET_SET, NET_FRAME, NET_STATS, NET_RX = 0x0401, 0x0402, 0x0403, 0x0480
HEADER = struct.Struct("<IHHII")
FRAGMENT = 1008
MAX_FRAME = 1518
# Credit is a byte allowance, not an allocation. Keep the outstanding allowance
# within the protocol's 32 KiB maximum and replenish only bytes actually consumed.
RX_WINDOW = 32768
RX_REFILL = RX_WINDOW // 2


class NetworkLink:
    def __init__(self, client, *, on_frame=None):
        self.client = client
        self.client.unsolicited = self.receive
        self.tx_id = 0
        self.rx_id = 0
        self.rx_sequence = 0
        self.partial = None
        self.frames = deque()
        # A frame handler runs in the receive path and must not block or call the client.
        self.on_frame = on_frame
        self.rx_dropped = 0
        self.credit = RX_WINDOW
        self.consumed = 0
        self.next_ping = 0

    def enable(self):
        self.update_credit()
        self.client.send(CHANNEL, NET_SET, b"\x01")

    def disable(self):
        self.client.send(CHANNEL, NET_SET, b"\x00")

    def update_credit(self):
        self.client.send(0, RX_LIMIT, struct.pack("<B3xQ", CHANNEL, self.credit))

    def stats(self):
        return struct.unpack("<IIIBB2x", self.client.send(CHANNEL, NET_STATS))

    def send(self, frame):
        if not 14 <= len(frame) <= MAX_FRAME:
            raise ValueError("Ethernet frame must have 14 to 1518 bytes without FCS")
        if self.tx_id == 0xFFFFFFFF:
            raise RuntimeError("open a new management session before Ethernet frame ID wrap")
        self.tx_id += 1
        crc = zlib.crc32(frame)
        for offset in range(0, len(frame), FRAGMENT):
            data = frame[offset:offset + FRAGMENT]
            payload = HEADER.pack(self.tx_id, offset, len(frame), crc, 0) + data
            status, detail, _ = self.client.exchange(CHANNEL, NET_FRAME, payload)
            if status in (4, 9):
                return False
            if status:
                raise RuntimeError(f"NET_FRAME status {status}, detail {detail}")
        return True

    def receive(self, message):
        kind, channel, op, _, seq, payload = message
        if (kind, channel, op) != (4, CHANNEL, NET_RX):
            return
        self.consumed += len(payload)
        if seq != self.rx_sequence + 1:
            if self.partial is not None:
                self.rx_dropped += 1
            self.partial = None
        self.rx_sequence = seq
        if len(payload) <= HEADER.size:
            self.rx_dropped += 1
            return
        frame_id, offset, length, crc, reserved = HEADER.unpack_from(payload)
        data = payload[HEADER.size:]
        if (not 14 <= length <= MAX_FRAME or offset >= length or
                len(data) != min(FRAGMENT, length - offset) or reserved):
            self.rx_dropped += 1
            self.partial = None
            return
        if offset == 0 and frame_id > self.rx_id:
            if self.partial is not None:
                self.rx_dropped += 1
            self.rx_id = frame_id
            self.partial = (length, crc, bytearray(), time.monotonic() + 1.0)
        if self.partial is None or frame_id != self.rx_id:
            self.rx_dropped += 1
            return
        total, expected_crc, frame, deadline = self.partial
        if offset != len(frame) or total != length or expected_crc != crc or time.monotonic() >= deadline:
            self.rx_dropped += 1
            self.partial = None
            return
        frame.extend(data)
        if len(frame) == length:
            self.partial = None
            if zlib.crc32(frame) != crc:
                self.rx_dropped += 1
            elif self.on_frame is not None:
                if not self.on_frame(bytes(frame)):
                    self.rx_dropped += 1
            elif len(self.frames) == 4:
                self.rx_dropped += 1
            else:
                self.frames.append(bytes(frame))

    def poll(self):
        self.client.poll(wait=False)
        if self.consumed >= RX_REFILL:
            self.credit += self.consumed
            self.consumed = 0
            self.update_credit()
        now = time.monotonic()
        if now >= self.next_ping:
            cookie = secrets.token_bytes(8)
            if self.client.send(0, PING, cookie)[:8] != cookie:
                raise ValueError("management PING cookie mismatch")
            self.next_ping = now + 0.5
