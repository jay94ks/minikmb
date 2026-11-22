"""Exercise MiniKMB's management CDC. Sends SYSTEM and DEBUG messages only."""

import argparse
import secrets
import struct
import time
import zlib

HEADER = struct.Struct("<HBBBBHQIHH")
HELLO, PING, INFO, STATUS, ECHO, RX_LIMIT = range(1, 7)
LOG_CONFIG, LOG_STATS, LOG_RECORD = 0x0501, 0x0502, 0x0580


def cobs_encode(raw):
    output = bytearray([0])
    code_at = 0
    code = 1
    for byte in raw:
        if byte:
            output.append(byte)
            code += 1
            if code < 255:
                continue
        output[code_at] = code
        code_at = len(output)
        output.append(0)
        code = 1
    output[code_at] = code
    return bytes(output)


def cobs_decode(encoded):
    output = bytearray()
    index = 0
    while index < len(encoded):
        code = encoded[index]
        end = index + code
        if not code or end > len(encoded):
            raise ValueError("bad COBS block")
        output.extend(encoded[index + 1:end])
        if code < 255 and end < len(encoded):
            output.append(0)
        index = end
    return bytes(output)


def request(channel, opcode, session, sequence, payload=b""):
    raw = HEADER.pack(0x4D4B, 1, 1, 0, channel, opcode, session,
                      sequence, len(payload), 0) + payload
    return cobs_encode(raw + struct.pack("<I", zlib.crc32(raw))) + b"\0"


def decode(encoded):
    raw = cobs_decode(encoded)
    if len(raw) < HEADER.size + 4:
        raise ValueError("short frame")
    magic, version, kind, flags, channel, opcode, session, seq, length, reserved = HEADER.unpack_from(raw)
    if (magic, version, flags, reserved) != (0x4D4B, 1, 0, 0) or len(raw) != HEADER.size + length + 4:
        raise ValueError("invalid frame header")
    if zlib.crc32(raw[:-4]) != int.from_bytes(raw[-4:], "little"):
        raise ValueError("CRC mismatch")
    return kind, channel, opcode, session, seq, raw[HEADER.size:-4]


class Client:
    def __init__(self, port, log_file, *, unsolicited=None):
        self.port = port
        self.log_file = log_file
        self.session = 0
        self.sequence = [1] * 6
        self.pending = bytearray()
        self.log_count = 0
        self.unsolicited = unsolicited

    def write(self, wire):
        # Empty COBS frames are ignored. End a full-packet write with a short packet.
        if wire and len(wire) % 64 == 0:
            wire += b"\0"
        if self.port.write(wire) != len(wire):
            raise OSError("short management CDC write")

    def read_frame(self):
        delimiter = self.pending.find(0)
        if delimiter < 0:
            self.pending.extend(self.port.read(max(1, self.port.in_waiting)))
            delimiter = self.pending.find(0)
        while delimiter == 0:
            del self.pending[0]
            delimiter = self.pending.find(0)
        if delimiter < 0:
            if len(self.pending) > 4096:
                raise ValueError("receive stream lost framing")
            return None
        encoded = bytes(self.pending[:delimiter])
        del self.pending[:delimiter + 1]
        return decode(encoded)

    def dispatch_unsolicited(self, frame):
        kind, channel, opcode, session, _, payload = frame
        if session != self.session:
            return
        if kind == 4 and channel == 5 and opcode == LOG_RECORD:
            stamp, log_seq, level, module, size = struct.unpack_from("<QIBBH", payload)
            if size != len(payload) - 16:
                raise ValueError("bad debug record length")
            line = f"{stamp:10} {log_seq:6} L{level} M{module} {payload[16:].decode('utf-8')}"
            print(line)
            if self.log_file is not None:
                print(line, file=self.log_file, flush=True)
            self.log_count += 1
        elif kind in (3, 4) and self.unsolicited is not None:
            self.unsolicited(frame)

    def poll(self, limit=16, *, wait=True):
        """Drain a bounded batch; wait=False never starts an empty serial read."""
        for _ in range(limit):
            if not wait and 0 not in self.pending and not self.port.in_waiting:
                break
            frame = self.read_frame()
            if frame is None:
                break
            self.dispatch_unsolicited(frame)

    def wait_reply(self, channel, opcode, sequence, timeout=1.0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            frame = self.read_frame()
            if frame is None:
                continue
            kind, received_channel, received_op, session, seq, payload = frame
            self.dispatch_unsolicited(frame)
            hello = channel == 0 and opcode == HELLO
            if (kind, received_channel, received_op, seq) == (2, channel, opcode, sequence) and (
                    hello or session == self.session):
                status, reserved, detail = struct.unpack_from("<HHI", payload)
                if reserved:
                    raise ValueError("bad response prefix")
                return status, detail, session, payload[8:]
        raise TimeoutError(f"response timeout for opcode {opcode:#06x}")

    def exchange(self, channel, opcode, payload=b"", *, attempts=3, timeout=1.0):
        seq = self.sequence[channel]
        wire = request(channel, opcode, self.session, seq, payload)
        for attempt in range(attempts):
            self.write(wire)
            try:
                status, detail, _, result = self.wait_reply(channel, opcode, seq, timeout)
                break
            except TimeoutError:
                if attempt + 1 == attempts:
                    raise
        if status not in (5, 6, 7, 8):
            self.sequence[channel] += 1
        return status, detail, result

    def send(self, channel, opcode, payload=b"", *, attempts=3):
        status, detail, result = self.exchange(channel, opcode, payload, attempts=attempts)
        if status:
            raise RuntimeError(f"device status {status}, detail {detail}, opcode {opcode:#06x}")
        return result

    def hello(self, uid):
        nonce = secrets.randbits(64) or 1
        wire = request(0, HELLO, 0, 1, struct.pack("<BBHQ", 1, 1, 0, nonce))
        for attempt in range(3):
            self.write(b"\0" + wire)
            status, detail, session, result = self.wait_reply(0, HELLO, 1)
            # The board can need one more RNG sample after rejecting session creation.
            # Replay the same nonce; do not conceal other errors or malformed replies.
            if (status, detail, session, result) != (9, 0, 0, b"") or attempt == 2:
                break
            time.sleep(0.05)
        if status or not session:
            raise RuntimeError(f"HELLO failed: {status}, {detail}")
        if len(result) != 40 or int.from_bytes(result[:8], "little") != nonce or result[8:20].hex().upper() != uid:
            raise ValueError("HELLO nonce or UID mismatch")
        self.session = session
        self.sequence = [1] * 6
        if self.send(0, INFO) != result:
            raise ValueError("INFO and HELLO disagree")
        return result

    def exercise(self, size):
        # Validate deduplication and verify that conflicting payloads do not replace a cached result.
        seq = self.sequence[0]
        self.send(0, ECHO, b"original")
        self.write(request(0, ECHO, self.session, seq, b"original"))
        assert self.wait_reply(0, ECHO, seq)[3] == b"original"
        self.write(request(0, ECHO, self.session, seq, b"conflict"))
        assert self.wait_reply(0, ECHO, seq)[0] == 7

        before = struct.unpack_from("<I", self.send(0, STATUS), 32)[0]
        raw = bytearray(cobs_decode(request(0, ECHO, self.session, self.sequence[0], b"test")[:-1]))
        raw[-1] ^= 1
        self.write(cobs_encode(raw) + b"\0")
        after = struct.unpack_from("<I", self.send(0, STATUS), 32)[0]
        assert after == (before + 1) & 0xFFFFFFFF

        self.send(0, RX_LIMIT, struct.pack("<B3xQ", 5, 4096))
        self.send(5, LOG_CONFIG, struct.pack("<BBHIHH", 1, 2, 0, 0x1FF, 20, 0))
        start = time.monotonic()
        last_ping = start
        remaining = size
        while remaining:
            if time.monotonic() - last_ping >= 0.5:
                cookie = secrets.token_bytes(8)
                assert self.send(0, PING, cookie)[:8] == cookie
                last_ping = time.monotonic()
            payload = secrets.token_bytes(min(remaining, 256))
            assert self.send(0, ECHO, payload) == payload
            remaining -= len(payload)
        self.send(5, LOG_STATS)
        print(f"{size} bytes each direction in {time.monotonic() - start:.3f} s")
        if self.log_count == 0:
            raise RuntimeError("no debug records received")


def select_port(uid, vid_pid):
    from serial.tools import list_ports

    identities = {vid_pid}
    if vid_pid == (0xCAFE, 0x4001):
        identities.update({(0xCAFE, 0x4002), (0xCAFE, 0x4004)})
    matches = [port for port in list_ports.comports() if (port.vid, port.pid) in identities
               and (port.serial_number or "").upper() == uid]
    if len(matches) != 1:
        raise RuntimeError(f"expected one matching MiniKMB port for {uid}; found {len(matches)}")
    return matches[0].device


def main():
    import serial

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--uid", required=True)
    parser.add_argument("--vid", type=lambda text: int(text, 0), default=0xCAFE)
    parser.add_argument("--pid", type=lambda text: int(text, 0), default=0x4001)
    parser.add_argument("--echo-bytes", type=int, default=1048576)
    parser.add_argument("--reopen", type=int, default=20, help="COM reopen count. Physical USB reset is separate.")
    parser.add_argument("--log", type=argparse.FileType("w", encoding="utf-8"))
    args = parser.parse_args()
    uid = args.uid.upper()
    if len(uid) != 24 or len(bytes.fromhex(uid)) != 12 or args.echo_bytes < 256 or args.reopen < 0:
        parser.error("invalid UID or count")
    previous_session = None
    for attempt in range(args.reopen + 1):
        port_name = select_port(uid, (args.vid, args.pid))
        with serial.Serial(port_name, 115200, timeout=0.03, write_timeout=1) as port:
            port.reset_input_buffer()
            client = Client(port, args.log)
            info = client.hello(uid)
            assert previous_session != client.session
            previous_session = client.session
            if attempt == 0:
                print(f"Verified HELLO on {port_name}, UID {uid}, features {int.from_bytes(info[28:32], 'little'):#x}")
                client.exercise(args.echo_bytes)
            else:
                cookie = secrets.token_bytes(8)
                assert client.send(0, PING, cookie)[:8] == cookie
    print(f"COM reopen and fresh session: {args.reopen}/{args.reopen}")


if __name__ == "__main__":
    main()
