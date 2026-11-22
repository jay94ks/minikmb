"""Inspect, mount or unmount the verified image on a MiniKMB optical drive."""

import argparse
import struct
import sys

from probe_management import Client, select_port
from upload_optical_image import IMAGE_INFO, STORAGE, Upload

IMAGE_MOUNT, IMAGE_UNMOUNT = 0x0306, 0x0307


def image_info(client):
    raw = client.send(STORAGE, IMAGE_INFO)
    image_id, size, kind, mounted, length, crc = struct.unpack_from("<IQBBHI", raw)
    if kind != 1 or mounted not in (0, 1) or len(raw) != 48 + length or raw[20:48] != bytes(28):
        raise ValueError("invalid optical image metadata")
    return image_id, size, bool(mounted), crc, raw[48:].decode("utf-8")


def main():
    import serial

    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("command", choices=("info", "mount", "unmount"))
    cli.add_argument("--uid", required=True, help="12-byte MiniKMB UID in hex")
    args = cli.parse_args()
    uid = args.uid.upper()
    if len(uid) != 24 or any(char not in "0123456789ABCDEF" for char in uid):
        cli.error("--uid must be exactly 24 hex digits")
    port = select_port(uid, (0xCAFE, 0x4001))
    with serial.Serial(port, timeout=0.05, write_timeout=1) as usb:
        client = Client(usb, None)
        hello = client.hello(uid)
        if not struct.unpack_from("<I", hello, 28)[0] & 0x10:
            raise RuntimeError("firmware does not expose a target optical drive")
        image_id, size, mounted, crc, name = image_info(client)
        if args.command != "info" and mounted != (args.command == "mount"):
            desired = args.command == "mount"
            job = Upload(client)
            opcode = IMAGE_MOUNT if desired else IMAGE_UNMOUNT
            job.job, = struct.unpack("<I", client.send(STORAGE, opcode, struct.pack("<I", image_id)))
            if job.wait_job(3 if desired else 4, 15) != image_id:
                raise ValueError("wrong image from media job")
            current = image_info(client)
            if current[0] != image_id or current[2] != desired:
                raise ValueError("media state did not match the job")
            mounted = desired
        print(f"{image_id} {'mounted' if mounted else 'ready'}, {size} bytes, CRC32 {crc:08x}, {name}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        sys.exit(str(error))
