"""Create and access a MiniKMB writable disk through the management CDC protocol."""

import argparse
from contextlib import contextmanager
from pathlib import Path
import struct
import sys
import tempfile
import time
import zlib

from prepare_writable_disk import SECTOR, changed_sectors
from probe_management import Client, PING, select_port
from upload_optical_image import STORAGE, SYSTEM, Upload

CREATE, COMMIT, OPEN, CLOSE, READ, WRITE, RESULT, SYNC, INFO = range(0x0310, 0x0319)
CREATE_JOB, COMMIT_JOB, OPEN_JOB, CLOSE_JOB, READ_JOB, WRITE_JOB, SYNC_JOB = range(7, 14)
FORMAT = {"fat32": 1, "exfat": 2, "ext4": 3}
DISK_LIMIT = 0x0FFFFFFF
USB_PATH = {"target": 1, "management": 2}
USB_STATE = {4: "target", 5: "management"}


class Disk:
    def __init__(self, client):
        self.client = client
        self.lease = 0
        self.ping_at = 0

    def heartbeat(self):
        now = time.monotonic()
        if now >= self.ping_at:
            cookie = struct.pack("<Q", time.monotonic_ns())
            reply = self.client.send(SYSTEM, PING, cookie)
            if reply[:8] != cookie:
                raise ValueError("unexpected management heartbeat response")
            self.ping_at = now + 0.5

    def run(self, opcode, kind, payload, timeout=30):
        job = Upload(self.client)
        job.job, = struct.unpack("<I", self.client.send(STORAGE, opcode, payload))
        started = job.job
        try:
            obj = job.wait_job(kind, timeout, 0.01 if kind in (READ_JOB, WRITE_JOB) else 0.25)
        except (OSError, ValueError, RuntimeError, KeyboardInterrupt):
            try:
                job.cancel()
            except (OSError, ValueError, RuntimeError):
                pass
            raise
        return started, obj

    def close(self):
        key = struct.pack("<I", self.lease)
        self.run(CLOSE, CLOSE_JOB, key)
        self.lease = 0

    @contextmanager
    def attached(self, disk_id, writable):
        _, self.lease = self.run(OPEN, OPEN_JOB, struct.pack("<IB3x", disk_id, writable))
        try:
            yield self
        except BaseException:
            try:
                self.close()
            except (OSError, ValueError, RuntimeError):
                pass
            raise
        else:
            self.close()

    def mount_usb(self, disk_id, side, writable):
        _, self.lease = self.run(OPEN, OPEN_JOB, struct.pack("<IBB2x", disk_id, writable, USB_PATH[side]))

    def active(self):
        raw = self.client.send(STORAGE, INFO)
        disk_id, sectors, lease, file_system, state, writable, reserved = struct.unpack("<IIIBBBB", raw)
        if not lease or reserved:
            raise ValueError("unexpected writable-disk metadata")
        return disk_id, sectors, file_system, state, bool(writable), lease

    def info(self):
        disk_id, sectors, file_system, state, writable, lease = self.active()
        if lease != self.lease:
            raise ValueError("unexpected writable-disk lease")
        return disk_id, sectors, file_system, state, writable

    def unmount_usb(self, expected_id):
        disk_id, _, _, state, _, lease = self.active()
        if disk_id != expected_id or state not in USB_STATE:
            raise ValueError("selected disk is not exported to USB")
        self.lease = lease
        self.close()

    def read(self, sector):
        job, _ = self.run(READ, READ_JOB, struct.pack("<II", self.lease, sector))
        data = b"".join(self.client.send(STORAGE, RESULT, struct.pack("<IIB", self.lease, job, part))
                        for part in range(2))
        if len(data) != SECTOR:
            raise ValueError("short sector response")
        return data

    def write(self, sector, data):
        if len(data) != SECTOR:
            raise ValueError("write requires exactly one 512-byte sector")
        self.run(WRITE, WRITE_JOB, struct.pack("<II", self.lease, sector) + data)

    def sync(self):
        self.run(SYNC, SYNC_JOB, struct.pack("<I", self.lease))

    def apply(self, original, edited, size):
        _, sectors, _, _, writable = self.info()
        if not writable or sectors * SECTOR != size:
            raise ValueError("image size or write access does not match disk")
        with original.open("rb") as before, original.open("rb") as sample, edited.open("rb") as after:
            with tempfile.TemporaryFile() as pending:
                count = 0
                for sector, data in changed_sectors(before, after, size, self.heartbeat):
                    actual = self.read(sector)
                    if actual == data:
                        continue
                    sample.seek(sector * SECTOR)
                    if actual != sample.read(SECTOR):
                        raise ValueError(f"sector {sector} differs from both images")
                    pending.write(struct.pack("<I", sector) + data)
                    count += 1
                pending.seek(0)
                while patch := pending.read(SECTOR + 4):
                    self.write(struct.unpack_from("<I", patch)[0], patch[4:])
                if count:
                    self.sync()
                    pending.seek(0)
                    while patch := pending.read(SECTOR + 4):
                        sector, = struct.unpack_from("<I", patch)
                        if self.read(sector) != patch[4:]:
                            raise ValueError(f"sector {sector} did not match after sync")
        return count

    def create(self, disk_id, file_system, stream, size, checksum, progress=print):
        _, self.lease = self.run(CREATE, CREATE_JOB, struct.pack("<IIB3x", disk_id, size // SECTOR,
                                                               file_system), 3600)
        try:
            count = 0
            for sector, data in changed_sectors(None, stream, size, self.heartbeat):
                self.write(sector, data)
                count += 1
                if count % 256 == 0:
                    progress(f"Sent {count} nonzero sectors")
            self.run(COMMIT, COMMIT_JOB, struct.pack("<II", self.lease, checksum), 3600)
            self.lease = 0
        except (OSError, ValueError, RuntimeError, KeyboardInterrupt):
            if self.lease:
                try:
                    self.close()
                except (OSError, ValueError, RuntimeError):
                    pass
            raise


def fingerprint(path):
    crc = 0
    length = 0
    with path.open("rb") as image:
        while block := image.read(1024 * 1024):
            crc = zlib.crc32(block, crc)
            length += len(block)
    if length == 0 or length % SECTOR or length // SECTOR > 0xFFFFFFFF:
        raise ValueError("invalid 512-byte disk image size")
    return length, crc


def main():
    import serial

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--uid", required=True, help="12-byte MiniKMB UID in hex")
    parser.add_argument("--disk-id", type=lambda value: int(value, 0))
    commands = parser.add_subparsers(dest="command", required=True)
    make = commands.add_parser("create")
    make.add_argument("image", type=Path)
    make.add_argument("--format", required=True, choices=tuple(FORMAT))
    commands.add_parser("info")
    mount = commands.add_parser("mount")
    mount.add_argument("side", choices=USB_PATH)
    mount.add_argument("--writable", action=argparse.BooleanOptionalAction,
                       help="default is writable on management and read-only on target")
    commands.add_parser("status")
    commands.add_parser("unmount", help="close after the OS has finished using the disk")
    apply = commands.add_parser("apply")
    apply.add_argument("original", type=Path)
    apply.add_argument("edited", type=Path)
    for action in ("read", "write"):
        command = commands.add_parser(action)
        command.add_argument("sector", type=int)
        command.add_argument("path", type=Path)
    args = parser.parse_args()
    uid = args.uid.upper()
    if len(uid) != 24 or any(char not in "0123456789ABCDEF" for char in uid):
        parser.error("--uid must be exactly 24 hex digits")
    if args.command != "status" and (args.disk_id is None or not 0 < args.disk_id <= DISK_LIMIT):
        parser.error("--disk-id must be between 1 and 0x0fffffff")
    if args.command in ("read", "write") and not 0 <= args.sector <= 0xFFFFFFFF:
        parser.error("invalid sector number")
    if args.command == "create":
        size, crc = fingerprint(args.image)
    if args.command == "apply":
        size = args.original.stat().st_size
        if not size or size % SECTOR or args.edited.stat().st_size != size:
            parser.error("both images must have the same 512-byte-aligned size")
    if args.command == "write":
        with args.path.open("rb") as stream:
            data = stream.read(SECTOR + 1)
        if len(data) != SECTOR:
            parser.error("input must be exactly 512 bytes")

    port = select_port(uid, (0xCAFE, 0x4001))
    with serial.Serial(port, timeout=0.05, write_timeout=1) as usb:
        client = Client(usb, None)
        client.hello(uid)
        disk = Disk(client)
        if args.command == "create":
            with args.image.open("rb") as stream:
                disk.create(args.disk_id, FORMAT[args.format], stream, size, crc)
            print(f"Created disk {args.disk_id:#x}, {size} bytes, CRC32 {crc:08x}")
        elif args.command == "mount":
            writable = args.writable if args.writable is not None else args.side == "management"
            disk.mount_usb(args.disk_id, args.side, writable)
            print(f"Mounted disk {args.disk_id:#x} on {args.side} ({'RW' if writable else 'RO'})")
        elif args.command == "status":
            disk_id, sectors, file_system, state, writable, _ = disk.active()
            name = next((name for name, number in FORMAT.items() if number == file_system), "unknown")
            side = USB_STATE.get(state, f"state {state}")
            print(f"Disk {disk_id:#x}: {sectors} sectors, {name}, {side}, {'RW' if writable else 'RO'}")
        elif args.command == "unmount":
            disk.unmount_usb(args.disk_id)
            print(f"Detached disk {args.disk_id:#x}")
        else:
            with disk.attached(args.disk_id, args.command in ("write", "apply")):
                if args.command == "info":
                    _, sectors, file_system, _, _ = disk.info()
                    name = next((name for name, number in FORMAT.items() if number == file_system), "unknown")
                    print(f"Disk {args.disk_id:#x}: {sectors} sectors, {name}")
                elif args.command == "read":
                    result = disk.read(args.sector)
                elif args.command == "apply":
                    count = disk.apply(args.original, args.edited, size)
                else:
                    disk.write(args.sector, data)
                    disk.sync()
            if args.command == "read":
                with args.path.open("xb") as output:
                    output.write(result)
                print(f"Saved sector {args.sector}")
            elif args.command == "write":
                print(f"Stored sector {args.sector}")
            elif args.command == "apply":
                print(f"Stored and verified {count} changed sectors")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        sys.exit(str(error))
    except KeyboardInterrupt:
        sys.exit(130)
