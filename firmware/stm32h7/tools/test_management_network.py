"""Exercise the NCM management wire without USB hardware."""
import ctypes as c
from pathlib import Path
import struct
import subprocess
import tempfile
import zlib

from network_link import CHANNEL, FRAGMENT, HEADER, NET_FRAME, NET_RX, NET_SET, NET_STATS, NetworkLink
from probe_management import Client, HEADER as WIRE_HEADER, RX_LIMIT, STATUS, cobs_encode
from test_management_hid import Fixture


def verify(libname):
    board = Fixture.__new__(Fixture)
    board.lib = c.CDLL(libname)
    lib = board.lib
    lib.Management_Input.argtypes = [c.c_uint8]
    lib.Management_Output.argtypes = [c.POINTER(c.c_size_t)]
    lib.Management_Output.restype = c.c_void_p
    lib.Management_OutputConsumed.argtypes = [c.c_size_t]
    lib.Management_Tick.argtypes = [c.c_uint64]
    for name in ("TestTarget", "TestUpload", "TestSpace"):
        getattr(lib, name).argtypes = [c.c_bool]
    lib.TestRx.argtypes = [c.c_char_p, c.c_uint16]
    lib.TestLast.argtypes = [c.c_void_p]
    lib.TestLast.restype = c.c_uint16
    lib.TestCommits.restype = c.c_uint32
    lib.TestInit()
    board.hello(1, features=0x60)
    next_seq = [1] * 6

    def command(op, payload=b"", channel=CHANNEL):
        seq = next_seq[channel]
        next_seq[channel] += 1
        return board.reply(channel, op, seq, payload)

    def poll():
        lib.Management_Poll()
        return board.output()

    def frame(frame_id, offset, crc=None):
        return HEADER.pack(frame_id, offset, len(data), zlib.crc32(data) if crc is None else crc, 0) + data[offset:offset + 1008]

    data = bytes(i % 251 for i in range(1518))
    assert command(NET_SET, b"\2")[0] == 2
    assert command(NET_SET, b"\1")[0] == 9
    lib.TestTarget(True)
    assert command(NET_SET, b"\1")[0] == 0
    assert command(STATUS, channel=0)[3][15] == 1
    assert poll()[2] == 0x0080  # Link-change event.
    lib.TestSpace(False)
    assert command(NET_FRAME, frame(1, 0))[0] == 4
    lib.TestSpace(True)
    assert command(NET_FRAME, frame(2, 0, 1))[0] == 0
    assert command(NET_FRAME, frame(2, 1008, 1))[0] == 12
    assert lib.TestCommits() == 0
    assert command(NET_FRAME, frame(3, 0))[0] == 0
    lib.Management_Tick(1001)
    assert command(NET_FRAME, frame(3, 1008))[0] == 3
    assert lib.TestCommits() == 0
    assert command(NET_FRAME, frame(4, 0))[0] == 0
    assert command(NET_FRAME, frame(4, 1008))[0] == 0
    assert board.reply(CHANNEL, NET_FRAME, next_seq[CHANNEL] - 1, frame(4, 1008))[0] == 0
    assert lib.TestCommits() == 1
    result = c.create_string_buffer(len(data))
    assert lib.TestLast(result) == len(data) and result.raw == data

    assert command(RX_LIMIT, struct.pack("<B3xQ", CHANNEL, 1024), 0)[0] == 0
    lib.TestRx(data, len(data))
    assert poll() is None
    assert struct.unpack("<IIIBB2x", command(NET_STATS)[3]) == (0, 0, 1, 1, 1)
    assert command(RX_LIMIT, struct.pack("<B3xQ", CHANNEL, 1550), 0)[0] == 0
    lib.TestRx(data, len(data))
    assert poll() == (4, CHANNEL, NET_RX, board.session, 1, frame(1, 0))
    assert poll() == (4, CHANNEL, NET_RX, board.session, 2, frame(1, 1008))
    lib.TestUpload(True)
    lib.Management_Tick(1002)
    assert command(NET_SET, b"\1")[0] == 3
    assert command(STATUS, channel=0)[3][15] == 0
    lib.TestUpload(False)
    assert command(NET_SET, b"\1")[0] == 0
    lib.Management_Tick(3003)
    board.hello(2, features=0x60)
    assert board.reply(CHANNEL, NET_STATS, 1)[3][12:14] == b"\0\0"
    print("NCM wire: fragmentation, CRC, queue backpressure, replay, credit and session teardown passed")


def verify_host_receive():
    frames = [bytes([index]) * 1400 for index in range(1, 9)]
    delivered = []

    def deliver(frame):
        delivered.append(frame)
        return frame != frames[-1]

    def enqueue(client, frame_id, data, crc):
        for offset in range(0, len(data), FRAGMENT):
            payload = HEADER.pack(frame_id, offset, len(data), crc, 0) + data[offset:offset + FRAGMENT]
            sequence = 2 * (frame_id - 1) + offset // FRAGMENT + 1
            header = WIRE_HEADER.pack(0x4D4B, 1, 4, 0, CHANNEL, NET_RX, client.session,
                                      sequence, len(payload), 0)
            raw = header + payload
            client.pending.extend(cobs_encode(raw + struct.pack("<I", zlib.crc32(raw))) + b"\0")

    for handler in (None, deliver):
        client = Client(None, None)
        client.session = 1
        link = NetworkLink(client, on_frame=handler)
        for frame_id, frame in enumerate(frames, 1):
            enqueue(client, frame_id, frame, zlib.crc32(frame))
        client.poll()  # One batch of 16 wire messages containing eight Ethernet frames.

        if handler is None:
            assert list(link.frames) == frames[:4] and link.rx_dropped == 4
        else:
            assert delivered == frames and not link.frames and link.rx_dropped == 1
            enqueue(client, 9, frames[0], zlib.crc32(frames[0]) ^ 1)
            client.poll(limit=2)
            assert delivered == frames and link.rx_dropped == 2

    print("NCM host receive: eight-frame burst, bounded fallback queue, handler rejection and CRC passed")


def verify_host_poll_and_credit():
    class Port:
        def __init__(self):
            self.buffer = bytearray()
            self.reads = 0

        @property
        def in_waiting(self):
            return len(self.buffer)

        def read(self, count):
            # A nonblocking poll must not issue an empty serial read.
            assert self.buffer
            self.reads += 1
            data = bytes(self.buffer[:count])
            del self.buffer[:count]
            return data

    port = Port()
    received = []
    client = Client(port, None, unsolicited=received.append)
    client.session = 1
    raw = WIRE_HEADER.pack(0x4D4B, 1, 4, 0, CHANNEL, NET_RX, 1, 1, 1, 0) + b'x'
    encoded = cobs_encode(raw + struct.pack('<I', zlib.crc32(raw))) + b'\0'
    client.poll(wait=False)
    assert port.reads == 0
    port.buffer.extend(encoded[:3])
    client.poll(wait=False)
    client.poll(wait=False)
    assert port.reads == 1 and not received
    port.buffer.extend(encoded[3:])
    client.poll(wait=False)
    assert port.reads == 2 and len(received) == 1 and received[0][-1] == b'x'
    client.pending.extend(encoded * 5)
    client.poll(limit=2, wait=False)
    assert len(received) == 3 and port.reads == 2
    client.poll(wait=False)
    assert len(received) == 6 and not client.pending and port.reads == 2

    class CreditClient:
        def __init__(self):
            self.commands = []
        def send(self, channel, opcode, payload=b''):
            self.commands.append((channel, opcode, payload))
            return b''
        def poll(self, *, wait):
            assert not wait

    from network_link import RX_WINDOW, RX_REFILL
    credit_client = CreditClient()
    link = NetworkLink(credit_client)
    link.next_ping = float('inf')
    link.enable()
    assert credit_client.commands[0] == (0, RX_LIMIT, struct.pack('<B3xQ', CHANNEL, 32768))
    link.consumed = RX_REFILL - 1
    link.poll()
    assert len(credit_client.commands) == 2
    link.consumed += 37
    consumed = link.consumed
    link.poll()
    assert len(credit_client.commands) == 3
    assert struct.unpack('<B3xQ', credit_client.commands[-1][2]) == (CHANNEL, RX_WINDOW + consumed)
    assert link.credit - consumed == 32768 and link.consumed == 0
    link.poll()
    assert len(credit_client.commands) == 3
    print('NCM host IO: idle/partial reads, bounded draining and consumed-byte credit batching passed')


if __name__ == "__main__":
    verify_host_poll_and_credit()
    verify_host_receive()
    root = Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory(prefix="minikmb-network-test-") as folder:
        library = str(Path(folder) / "network.so")
        subprocess.run(["gcc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-shared", "-fPIC",
                        "-I", "Core/Inc", "Core/Src/usb/management_protocol.c",
                        "tools/test_management_network_fixture.c", "-o", library], cwd=root, check=True)
        verify(library)
