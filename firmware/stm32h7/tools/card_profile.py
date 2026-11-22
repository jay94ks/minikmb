"""Query or initialize MiniKMB card identity and transfer its opaque user value."""

import argparse
from pathlib import Path
import json
import struct
import sys
import uuid

from probe_management import Client, select_port
from upload_optical_image import STORAGE, Upload

CARD_INFO, CARD_INIT, USER_READ, USER_BEGIN, USER_DATA, USER_COMMIT, USER_ABORT = range(0x0308, 0x030F)
INFO_RECORD = struct.Struct("<BBHI16s12s12sHH")
CARD_FORMAT = 0x030F
FORMAT_JOB = 14
CARD_FILESYSTEM = {"fat32": 1, "exfat": 2}
USER_LIMIT = 4096
DATA_CHUNK = 1008


def card_info(client):
    flags, reserved, length, revision, board, created, live, profile_status, user_status = (
        INFO_RECORD.unpack(client.send(STORAGE, CARD_INFO))
    )
    if reserved or flags & ~7 or length > USER_LIMIT:
        raise ValueError("invalid card profile response")
    return {
        "card_present": bool(flags & 1),
        "board_id": str(uuid.UUID(bytes=board)) if flags & 2 else None,
        "registered_controller": created.hex().upper() if flags & 2 else None,
        "controller_uid": live.hex().upper(),
        "user_defined": bool(flags & 4),
        "user_bytes": length,
        "user_revision": revision,
        "profile_status": profile_status,
        "user_status": user_status,
    }


def run_job(client, opcode, kind, payload):
    job = Upload(client)
    job.job, = struct.unpack("<I", client.send(STORAGE, opcode, payload))
    completed = job.wait_job(kind, 30)
    return completed


def initialize(client, board_id):
    run_job(client, CARD_INIT, 5, board_id.bytes)
    info = card_info(client)
    if info["board_id"] != str(board_id):
        raise ValueError("initialized board ID did not match")
    return info


def format_card(client, filesystem):
    job = Upload(client)
    job.job, = struct.unpack("<I", client.send(STORAGE, CARD_FORMAT,
                                               struct.pack("<B3x", CARD_FILESYSTEM[filesystem]), attempts=1))
    print(f"Card format job {job.job} accepted", flush=True)
    job.wait_job(FORMAT_JOB, 120)
    info = card_info(client)
    if (not info["card_present"] or info["board_id"] is None or info["user_defined"]
            or info["profile_status"] != 0 or info["user_status"] != 17):
        raise ValueError("formatted card did not automatically prepare its internal identity")


def read_user(client):
    info = card_info(client)
    if not info["user_defined"]:
        raise RuntimeError(f"no readable user-defined value, status {info['user_status']}")
    data = bytearray()
    while len(data) < info["user_bytes"]:
        length = min(256, info["user_bytes"] - len(data))
        payload = struct.pack("<IHH", info["user_revision"], len(data), length)
        chunk = client.send(STORAGE, USER_READ, payload)
        if len(chunk) != length:
            raise ValueError("short user-value response")
        data.extend(chunk)
    end = card_info(client)
    if end["user_revision"] != info["user_revision"]:
        raise RuntimeError("user-defined value changed during read")
    return data


def write_user(client, data):
    if len(data) > USER_LIMIT:
        raise ValueError("user-defined value exceeds 4096 bytes")
    transfer = client.send(STORAGE, USER_BEGIN, struct.pack("<H", len(data)))
    if len(transfer) != 4:
        raise ValueError("invalid user transfer ID")
    try:
        for offset in range(0, len(data), DATA_CHUNK):
            chunk = data[offset:offset + DATA_CHUNK]
            accepted, = struct.unpack("<H", client.send(
                STORAGE, USER_DATA, transfer + struct.pack("<H", offset) + chunk,
            ))
            if accepted != offset + len(chunk):
                raise ValueError("user transfer acknowledged the wrong offset")
        run_job(client, USER_COMMIT, 6, transfer)
    except (OSError, RuntimeError, ValueError, KeyboardInterrupt):
        try:
            client.send(STORAGE, USER_ABORT, transfer, attempts=1)
        except (OSError, RuntimeError, ValueError):
            pass
        raise
    if read_user(client) != data:
        raise ValueError("stored user-defined value did not match")


def main():
    import serial

    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--uid", required=True, help="12-byte MiniKMB UID in hex")
    sub = cli.add_subparsers(dest="command", required=True)
    sub.add_parser("info")
    sub.add_parser("init").add_argument("board_id", type=uuid.UUID)
    sub.add_parser("read-user").add_argument("path", type=Path)
    sub.add_parser("write-user").add_argument("path", type=Path)
    fmt = sub.add_parser("format", help="Replace the selected microSD with a new filesystem")
    fmt.add_argument("filesystem", choices=CARD_FILESYSTEM)
    fmt.add_argument("--erase", action="store_true", required=True, help="Erase all files on this card")
    args = cli.parse_args()
    uid = args.uid.upper()
    if len(uid) != 24 or any(char not in "0123456789ABCDEF" for char in uid):
        cli.error("--uid must be exactly 24 hex digits")

    data = None
    if args.command == "write-user":
        with args.path.open("rb") as stream:
            data = stream.read(USER_LIMIT + 1)
        if len(data) > USER_LIMIT:
            cli.error("user-defined value exceeds 4096 bytes")

    port = select_port(uid, (0xCAFE, 0x4001))
    with serial.Serial(port, timeout=0.05, write_timeout=1) as usb:
        client = Client(usb, None)
        client.hello(uid)
        if args.command == "info":
            print(json.dumps(card_info(client), indent=2))
        elif args.command == "init":
            print(json.dumps(initialize(client, args.board_id), indent=2))
        elif args.command == "read-user":
            data = read_user(client)
            with args.path.open("xb") as stream:
                stream.write(data)
            print(f"Saved {len(data)} bytes")
        elif args.command == "format":
            format_card(client, args.filesystem)
            print(f"Formatted and remounted {uid} microSD as {args.filesystem}")
        else:
            write_user(client, data)
            print(f"Stored and verified {len(data)} bytes")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        sys.exit(str(error))
    except KeyboardInterrupt:
        sys.exit(130)
