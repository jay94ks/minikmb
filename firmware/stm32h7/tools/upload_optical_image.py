"""Upload a prepared optical image through the MiniKMB management CDC."""

import argparse
from itertools import islice
from pathlib import Path
import struct
import sys
import time

from prepare_optical_image import inspect_image, upload_chunks
from probe_management import Client, STATUS, request, select_port

SYSTEM, STORAGE = 0, 3
JOB_STATUS, JOB_CANCEL, UPLOAD_CANCEL = 0x0007, 0x0008, 0x0009
BEGIN, DATA, COMMIT, IMAGE_INFO = 0x0301, 0x0302, 0x0303, 0x0305
JOB_RESULT = struct.Struct("<IBBHIIQQ")
BUSY, GAP, RANGE = 4, 8, 10
DATA_WINDOW_MAX = 4
DATA_RESPONSE = struct.Struct("<HHI")


class Upload:
    def __init__(self, client, progress=print, *, data_window=1):
        if not 1 <= data_window <= DATA_WINDOW_MAX:
            raise ValueError("invalid upload request window")
        self.client = client
        self.data_window = data_window
        self.progress = progress
        self.job = 0
        self.upload = 0

    def wait_job(self, kind, timeout, poll_interval=0.25):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            reply = self.client.send(SYSTEM, JOB_STATUS, struct.pack("<I", self.job))
            job, state, received_kind, status, detail, obj, _, _ = JOB_RESULT.unpack(reply)
            if job != self.job or received_kind != kind:
                raise ValueError("unexpected job response")
            if state == 1 and status == 0 and obj != 0:
                self.job = 0
                return obj
            if state != 0:
                raise RuntimeError(f"job {job} failed with status {status}, detail {detail}")
            time.sleep(poll_interval)
        raise TimeoutError(f"job {self.job} did not finish within {timeout:.0f} seconds")

    def cancel(self):
        if self.job:
            self.client.send(SYSTEM, JOB_CANCEL, struct.pack("<I", self.job), attempts=1)
        elif self.upload:
            self.client.send(SYSTEM, UPLOAD_CANCEL, struct.pack("<I", self.upload), attempts=1)

    def exchange_data(self, chunks, deadline):
        if self.data_window == 1:
            return [self.client.exchange(STORAGE, DATA, chunks[0])]

        client = self.client
        base = client.sequence[STORAGE]
        if base + len(chunks) >= 0xffffffff:
            raise RuntimeError("STORAGE sequence would wrap before COMMIT")
        pending = {base + index: request(STORAGE, DATA, client.session, base + index, chunk)
                   for index, chunk in enumerate(chunks)}
        replies = {}
        while pending:
            if time.monotonic() >= deadline:
                raise TimeoutError(f"unconfirmed DATA sequence {min(pending)}")
            wire = b"".join(pending.values())
            client.write(wire)
            reply_deadline = min(deadline, time.monotonic() + 0.28)
            while pending and time.monotonic() < reply_deadline:
                frame = client.read_frame()
                if frame is None:
                    continue
                client.dispatch_unsolicited(frame)
                kind, channel, op, session, seq, payload = frame
                if (kind, channel, op, session) != (2, STORAGE, DATA, client.session) or seq not in pending:
                    continue
                if len(payload) < DATA_RESPONSE.size:
                    raise ValueError("short DATA response")
                status, reserved, detail = DATA_RESPONSE.unpack_from(payload)
                if reserved or status in (5, 6, 7):
                    raise RuntimeError(f"unsafe DATA replay state at sequence {seq}: {status}, {detail}")
                if status == GAP:
                    if not base <= detail < seq:
                        raise ValueError(f"unrecoverable DATA sequence gap {detail} before {seq}")
                    continue
                replies[seq] = (status, detail, payload[DATA_RESPONSE.size:])
                del pending[seq]

        client.sequence[STORAGE] += len(chunks)
        return [replies[base + index] for index in range(len(chunks))]

    def send_image(self, stream, image, name, verify_timeout):
        try:
            self.job, = struct.unpack("<I", self.client.send(STORAGE, BEGIN, image.begin_payload(name)))
            self.upload = self.wait_job(1, 90)
            offset, next_report = 0, 8 * 1024 * 1024
            chunks = iter(upload_chunks(stream, image, self.upload))
            while group := list(islice(chunks, self.data_window)):
                deadline = time.monotonic() + 6
                while group:
                    replies = self.exchange_data(group, deadline)
                    for index, (status, detail, reply) in enumerate(replies):
                        if status == BUSY:
                            if any(later[0] != RANGE for later in replies[index + 1:]):
                                raise ValueError("unexpected DATA reply beyond the busy offset")
                            if time.monotonic() >= deadline:
                                raise TimeoutError("SD staging buffer remained full")
                            group = group[index:]
                            time.sleep(max(1, min(detail, 250)) / 1000)
                            break
                        if status != 0:
                            raise RuntimeError(f"upload DATA failed at {offset}, status {status}, detail {detail}")
                        offset += len(group[index]) - 12
                        if len(reply) != 8 or int.from_bytes(reply, "little") != offset:
                            raise ValueError("device acknowledged the wrong image offset")
                    else:
                        group = []
                if offset >= next_report or offset == image.size:
                    self.progress(f"Accepted {offset}/{image.size} bytes")
                    next_report = offset + 8 * 1024 * 1024
            if offset != image.size:
                raise ValueError("incomplete optical upload")
            self.job, = struct.unpack("<I", self.client.send(STORAGE, COMMIT, struct.pack("<I", self.upload)))
            image_id = self.wait_job(2, verify_timeout)
            self.upload = 0
        except (OSError, RuntimeError, ValueError, KeyboardInterrupt):
            try:
                self.cancel()
            except (OSError, RuntimeError, ValueError):
                pass  # A broken management link also expires the device upload lease.
            raise

        reply = self.client.send(STORAGE, IMAGE_INFO)
        actual_id, size, kind, mounted, name_len, crc = struct.unpack_from("<IQBBHI", reply)
        if (actual_id != image_id or size != image.size or kind != 1 or mounted != 0 or
                crc != image.crc32 or reply[20:48] != bytes(28) or
                name_len != len(reply) - 48 or reply[48:].decode("utf-8") != name):
            raise ValueError("committed image metadata disagrees with the uploaded image")
        return image_id


def main():
    import serial

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("--uid", required=True, help="12-byte MiniKMB UID in hex")
    parser.add_argument("--verify-timeout", type=float, default=3600)
    args = parser.parse_args()
    uid = args.uid.upper()
    if len(uid) != 24 or any(char not in "0123456789ABCDEF" for char in uid):
        parser.error("--uid must be exactly 24 hex digits")
    if not 0 < args.verify_timeout < float("inf"):
        parser.error("--verify-timeout must be positive and finite")

    with args.image.open("rb") as stream:
        image = inspect_image(stream)
        port = select_port(uid, (0xCAFE, 0x4001))
        with serial.Serial(port, timeout=0.05, write_timeout=1) as usb:
            client = Client(usb, None)
            info = client.hello(uid)
            if not struct.unpack_from("<I", info, 28)[0] & 4:
                raise RuntimeError("firmware does not expose image upload")
            status = client.send(SYSTEM, STATUS)
            if status[13] == 0:
                raise RuntimeError("no microSD card detected")
            window = min(DATA_WINDOW_MAX, info[36]) or 1
            image_id = Upload(client, data_window=window).send_image(
                stream, image, args.image.name, args.verify_timeout,
            )
        print(f"Verified image {image_id}, {image.size} bytes, CRC32 {image.crc32:08x}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        sys.exit(str(error))
    except KeyboardInterrupt:
        sys.exit(130)
