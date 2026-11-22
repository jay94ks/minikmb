"""Compare every byte read from the mounted MiniKMB optical drive with its image."""

import argparse
from contextlib import contextmanager
from pathlib import Path
import struct
import sys
import time
import zlib



@contextmanager
def open_optical(device, image_size):
    if sys.platform != "win32" or not device.startswith(chr(92) * 2):
        with open(device, "rb", buffering=0) as optical:
            yield optical.read
        return

    import ctypes as c
    from ctypes import wintypes as wt

    ioctl_scsi_pass_through_direct = 0x0004D014

    class SptDirect(c.Structure):
        _fields_ = [
            ("length", wt.USHORT), ("status", wt.BYTE), ("path_id", wt.BYTE),
            ("target_id", wt.BYTE), ("lun", wt.BYTE), ("cdb_length", wt.BYTE),
            ("sense_length", wt.BYTE), ("direction", wt.BYTE), ("transfer", wt.ULONG),
            ("timeout", wt.ULONG), ("buffer", wt.LPVOID), ("sense_offset", wt.ULONG),
            ("cdb", c.c_ubyte * 16),
        ]

    class Packet(c.Structure):
        _fields_ = [("request", SptDirect), ("sense", c.c_ubyte * 32)]

    kernel = c.WinDLL("kernel32", use_last_error=True)
    kernel.CreateFileW.argtypes = (wt.LPCWSTR, wt.DWORD, wt.DWORD, wt.LPVOID, wt.DWORD, wt.DWORD, wt.HANDLE)
    kernel.CreateFileW.restype = wt.HANDLE
    kernel.DeviceIoControl.argtypes = (wt.HANDLE, wt.DWORD, wt.LPVOID, wt.DWORD, wt.LPVOID,
                                      wt.DWORD, c.POINTER(wt.DWORD), wt.LPVOID)
    kernel.DeviceIoControl.restype = wt.BOOL
    kernel.CloseHandle.argtypes = (wt.HANDLE,)

    # SPTI uses read/write handle access even for READ CAPACITY and READ(10).
    handle = kernel.CreateFileW(device, 0xC0000000, 3, None, 3, 0, None)
    if handle in (None, c.c_void_p(-1).value):
        raise c.WinError(c.get_last_error())
    buffer = c.create_string_buffer(64 * 1024 + 4095)
    aligned = (c.addressof(buffer) + 4095) & ~4095

    def command(cdb, transfer):
        packet = Packet()
        request = packet.request
        request.length = c.sizeof(SptDirect)
        request.cdb_length = len(cdb)
        request.sense_length = len(packet.sense)
        request.direction = 1
        request.transfer = transfer
        request.timeout = 8
        request.buffer = aligned
        request.sense_offset = Packet.sense.offset
        request.cdb[:len(cdb)] = cdb
        received = wt.DWORD()
        if not kernel.DeviceIoControl(handle, ioctl_scsi_pass_through_direct,
                                      c.byref(packet), c.sizeof(packet), c.byref(packet),
                                      c.sizeof(packet), c.byref(received), None):
            raise c.WinError(c.get_last_error())
        if request.status:
            sense = bytes(packet.sense[:request.sense_length]).hex(" ")
            raise OSError(f"SCSI {cdb[0]:02x} status {request.status:02x}, sense {sense}")
        return c.string_at(aligned, request.transfer)

    lba = 0

    def read(length):
        nonlocal lba
        blocks = length // 2048
        opcode = struct.pack(">BBIBHB", 0x28, 0, lba, 0, blocks, 0)
        data = command(opcode, length)
        lba += blocks
        return data

    try:
        capacity = command(bytes([0x25]) + bytes(9), 8)
        max_lba, logical_block = struct.unpack(">II", capacity)
        if logical_block != 2048 or image_size != (max_lba + 1) * logical_block:
            raise ValueError(f"wrong optical capacity: {max_lba + 1} blocks of {logical_block} bytes")
        yield read
    finally:
        kernel.CloseHandle(handle)


def verify(image, device):
    size = image.stat().st_size
    if size == 0 or size % 2048:
        raise ValueError("expected a nonempty 2,048-byte-block image")
    started = time.monotonic()
    crc = 0
    offset = 0
    with image.open("rb") as original, open_optical(device, size) as read:
        while expected := original.read(64 * 1024):
            actual = read(len(expected))
            if len(actual) != len(expected):
                raise OSError(f"short optical read at {offset}: {len(actual)}/{len(expected)} bytes")
            if actual != expected:
                mismatch = next(index for index, (left, right) in enumerate(zip(actual, expected))
                                if left != right)
                raise ValueError(f"optical image differs at byte offset {offset + mismatch}")
            offset += len(actual)
            crc = zlib.crc32(actual, crc)
    if offset != size:
        raise ValueError("reference image length changed during verification")
    return offset, crc, time.monotonic() - started


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("image", type=Path)
    cli.add_argument("device", help=r"raw optical path, e.g. \\.\D: on Windows or /dev/sr0 on Linux")
    args = cli.parse_args()
    size, crc, duration = verify(args.image, args.device)
    print(f"{size} optical bytes match, CRC32 {crc:08x}, {duration:.3f} seconds")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        sys.exit(str(error))
