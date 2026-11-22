"""Compile and exercise the M4 management wire path with a simulated USB target."""

import argparse
import ctypes
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

from probe_management import HELLO, STATUS, cobs_decode, cobs_encode, decode, request


KEY_DOWN, KEY_UP, MOVE, BUTTON_DOWN, BUTTON_UP, WHEEL, RELEASE_ALL = range(0x0101, 0x0108)
HID_DONE = 0x0180
HID = 1


class Fixture:
    def __init__(self, library):
        self.lib = ctypes.CDLL(str(library))
        self.lib.Test_Init.argtypes = [ctypes.c_bool]
        self.lib.Test_Storage.argtypes = [ctypes.c_uint8]
        self.lib.Test_SetConnected.argtypes = [ctypes.c_bool]
        self.lib.Test_TakeReport.argtypes = [ctypes.POINTER(ctypes.c_uint8)]
        self.lib.Test_TakeReport.restype = ctypes.c_uint64
        self.lib.Test_Complete.argtypes = [ctypes.c_uint64, ctypes.c_bool]
        self.lib.Management_Input.argtypes = [ctypes.c_uint8]
        self.lib.Management_Output.argtypes = [ctypes.POINTER(ctypes.c_size_t)]
        self.lib.Management_Output.restype = ctypes.c_void_p
        self.lib.Management_OutputConsumed.argtypes = [ctypes.c_size_t]
        self.lib.Management_Tick.argtypes = [ctypes.c_uint64]
        for name in ("Test_Init", "Test_Storage", "Test_SetConnected", "Test_Complete",
                     "Management_Input", "Management_OutputConsumed", "Management_Tick",
                     "Management_Poll"):
            getattr(self.lib, name).restype = None

    def start(self, enabled=True):
        self.lib.Test_Init(enabled)
        self.session = 0
        self.lib.Test_SetConnected(True)

    def output(self):
        size = ctypes.c_size_t()
        payload = self.lib.Management_Output(ctypes.byref(size))
        if not size.value:
            return None
        wire = ctypes.string_at(payload, size.value)
        self.lib.Management_OutputConsumed(size)
        assert wire[-1] == 0
        return decode(wire[:-1])

    def feed(self, wire):
        for byte in wire:
            self.lib.Management_Input(byte)

    def reply(self, channel, opcode, sequence, payload=b"", *, consume=True):
        self.feed(request(channel, opcode, self.session, sequence, payload))
        return self.response(channel, opcode, sequence) if consume else None

    def response(self, channel, opcode, sequence):
        kind, found_channel, op, uid, seq, payload = self.output()
        assert (kind, found_channel, op, seq) == (2, channel, opcode, sequence)
        if opcode != HELLO:
            assert uid == self.session
        status, reserved, detail = struct.unpack_from("<HHI", payload)
        assert reserved == 0
        return status, detail, uid, payload[8:]

    def hello(self, nonce, features=0x41):
        self.session = 0
        status, detail, session, info = self.reply(0, HELLO, 1, struct.pack("<BBHQ", 1, 1, 0, nonce))
        assert status == detail == 0 and session != 0
        assert len(info) == 40 and info[8:10] == b"\xcd\x12"
        assert struct.unpack_from("<I", info, 28)[0] == features
        self.session = session
        return session

    def take(self):
        report = (ctypes.c_uint8 * 16)()
        transfer = self.lib.Test_TakeReport(report)
        return transfer, bytes(report)

    def neutral(self):
        transfer, report = self.take()
        assert transfer and report[0] == 1 and report[1:9] == bytes(8)
        self.lib.Test_Complete(transfer, True)
        transfer, report = self.take()
        assert transfer and report[0] == 2 and report[9] == 0
        self.lib.Test_Complete(transfer, True)

    def event(self, sequence, ticket, status=0):
        self.lib.Management_Poll()
        frame = self.output()
        assert frame == (3, HID, HID_DONE, self.session, sequence, struct.pack("<IHH", ticket, status, 0))

    def no_event(self):
        self.lib.Management_Poll()
        assert self.output() is None


def ticket(reply):
    status, detail, _, payload = reply
    assert status == detail == 0 and len(payload) == 4
    value, = struct.unpack("<I", payload)
    assert value != 0
    return value


def test_replay(fixture):
    fixture.start(False)
    fixture.hello(1, 0x40)
    assert fixture.reply(HID, KEY_DOWN, 1, b"\x04\0")[:2] == (1, KEY_DOWN)
    fixture.start()
    fixture.hello(2)
    fixture.neutral()

    damaged = bytearray(cobs_decode(request(HID, KEY_DOWN, fixture.session, 1, b"\x04\0")[:-1]))
    damaged[-1] ^= 1
    fixture.feed(cobs_encode(damaged) + b"\0")
    assert fixture.output() is None and fixture.take()[0] == 0

    original = fixture.reply(HID, KEY_DOWN, 1, b"\x04\0")
    key_ticket = ticket(original)
    assert fixture.reply(HID, KEY_DOWN, 1, b"\x04\0") == original
    assert fixture.reply(HID, KEY_DOWN, 1, b"\x05\0")[:2] == (7, 0)
    fixture.no_event()
    transfer, report = fixture.take()
    assert transfer and report[0] == 1 and report[1:9] == bytes([0, 0, 4, 0, 0, 0, 0, 0])
    fixture.lib.Test_Complete(transfer, True)
    # A staged STATUS response must precede the waiting event.
    fixture.reply(0, STATUS, 1, consume=False)
    fixture.lib.Management_Poll()
    assert fixture.response(0, STATUS, 1)[:2] == (0, 0)
    fixture.event(1, key_ticket)
    assert fixture.take()[0] == 0
    assert fixture.reply(HID, KEY_UP, 2, b"\x04")[:2] == (2, 0)

    wheel_payload = struct.pack("<hh", -127, 127)
    wheel_reply = fixture.reply(HID, WHEEL, 3, wheel_payload)
    assert fixture.reply(HID, WHEEL, 3, wheel_payload) == wheel_reply
    transfer, report = fixture.take()
    assert transfer and report[0] == 2 and report[14:16] == bytes([129, 127])
    fixture.lib.Test_Complete(transfer, True)
    fixture.event(2, ticket(wheel_reply))
    assert fixture.take()[0] == 0
    fixture.no_event()


def test_release_and_overflow(fixture):
    fixture.start()
    fixture.hello(3)
    fixture.neutral()
    # Each interface reserves 16 slots; all 32 accepted tickets are cancelled.
    mouse = [ticket(fixture.reply(HID, WHEEL, seq, b"\x01\0\0\0")) for seq in range(1, 17)]
    keyboard = [ticket(fixture.reply(HID, KEY_DOWN, seq, b"\x04\0")) for seq in range(17, 33)]
    accepted = keyboard + mouse
    assert fixture.reply(HID, WHEEL, 33, b"\x01\0\0\0")[:2] == (4, 10)
    release = ticket(fixture.reply(HID, RELEASE_ALL, 34))
    status = fixture.reply(0, STATUS, 1)[3]
    assert struct.unpack_from("<I", status, 60)[0] == 0
    for event_sequence, cancelled in enumerate(accepted, 1):
        fixture.event(event_sequence, cancelled, 13)
    fixture.no_event()
    fixture.neutral()
    fixture.event(33, release)
    assert fixture.take()[0] == 0


def test_session_and_storage(fixture):
    fixture.start()
    old_session = fixture.hello(4)
    fixture.neutral()
    assert ticket(fixture.reply(HID, KEY_DOWN, 1, b"\x04\0"))
    active, _ = fixture.take()
    assert active
    assert ticket(fixture.reply(HID, BUTTON_DOWN, 2, b"\x01"))

    fixture.lib.Management_Tick(2000)
    assert fixture.reply(HID, KEY_UP, 3, b"\x04\0")[:2] == (5, 0)
    assert fixture.hello(5) != old_session
    fixture.lib.Test_Complete(active, True)
    fixture.neutral()
    fixture.no_event()
    assert fixture.take()[0] == 0

    fixture.lib.Test_Storage(3)
    assert fixture.reply(HID, KEY_DOWN, 1, b"\x04\0")[:2] == (3, 3)
    release = ticket(fixture.reply(HID, RELEASE_ALL, 2))
    fixture.neutral()
    fixture.event(1, release)
    fixture.lib.Test_Storage(1)
    fixture.lib.Test_SetConnected(False)
    assert fixture.reply(HID, KEY_DOWN, 3, b"\x04\0")[:2] == (9, 1)
    release = ticket(fixture.reply(HID, RELEASE_ALL, 4))
    fixture.lib.Test_SetConnected(True)
    fixture.neutral()
    fixture.event(2, release)


def main(compiler):
    project = Path(__file__).resolve().parent.parent
    sources = (
        "Core/Src/usb/management_protocol.c",
        "Core/Src/usb/management_hid.c",
        "Core/Src/usb/target_hid_report.c",
        "Core/Src/usb/target_hid_queue.c",
        "tools/test_management_hid_fixture.c",
    )
    with tempfile.TemporaryDirectory() as work:
        library = Path(work) / ("management_hid.dll" if sys.platform == "win32" else "management_hid.so")
        subprocess.run(
            [compiler, "-std=c11", "-O2", "-Wall", "-Wextra", "-Wpedantic", "-Werror",
             "-shared", "-fPIC", "-I", str(project / "Core/Inc"),
             *(str(project / name) for name in sources), "-o", str(library)],
            check=True,
        )
        # Run in another process so Windows can release the loaded test DLL before cleanup.
        subprocess.run([sys.executable, __file__, "--library", str(library)], check=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cc", default="gcc", help="native GCC compiler")
    parser.add_argument("--library", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.library is not None:
        fixture = Fixture(args.library)
        test_replay(fixture)
        test_release_and_overflow(fixture)
        test_session_and_storage(fixture)
        print("HID wire replay, event priority, overflow, expiry, storage and reconnect tests passed.")
    else:
        main(args.cc)
