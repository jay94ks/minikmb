"""Exercise a fixed OS command across the physical MiniKMB CDC bridge."""

import argparse
import re
import secrets
import struct
import subprocess
import sys
import time
from pathlib import Path

import serial

from probe_management import Client, select_port


PYTHON_VERSION = re.compile(rb"Python [0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.+-]*")
REQUEST = re.compile(rb"MKB1 ([0-9a-f]{16}) ([A-Z_]{1,24})")
SERIAL = 2
SERIAL_RX = 0x0280
SERIAL_STATUS = 0x0202
SERIAL_WRITE = 0x0201


def version_command():
    return subprocess.check_output(
        [sys.executable, "-I", "-B", "--version"],
        stderr=subprocess.STDOUT,
        stdin=subprocess.DEVNULL,
        timeout=3,
    ).strip()


def target(uid, target_pid):
    name = select_port(uid, (0xCAFE, target_pid))
    deadline = time.monotonic() + 25
    completed = 0
    rejected = 0
    pending = bytearray()
    committed = False
    with serial.Serial(name, 57600, timeout=0.04, write_timeout=1) as port:
        port.reset_input_buffer()
        print(f"target ready on {name}", flush=True)
        while not committed and time.monotonic() < deadline:
            pending.extend(port.read(max(1, port.in_waiting)))
            while b"\n" in pending:
                frame, _, pending = pending.partition(b"\n")
                match = REQUEST.fullmatch(frame)
                if not match:
                    raise ValueError("invalid command fixture frame")
                token, command = match.groups()
                if command == b"DONE" and (completed, rejected) == (2, 1):
                    committed = True
                    break
                if command == b"VERSION" and completed < 2:
                    payload = b"OK " + version_command()
                    completed += 1
                else:
                    payload = b"ERR unsupported"
                    rejected += 1
                reply = b"MKB1 " + token + b" " + payload + b"\n"
                if port.write(reply) != len(reply):
                    raise IOError("incomplete target reply")
            if len(pending) > 64:
                raise ValueError("incomplete command exceeded maximum length")
        port.flush()
        if not committed or (completed, rejected) != (2, 1):
            raise RuntimeError(f"target command verification incomplete: {completed}, {rejected}")
        print(f"target executed {completed} commands and rejected {rejected}", flush=True)


def management(uid, expected_version):
    pending = bytearray()
    received_sequence = 0

    def consume(frame):
        nonlocal received_sequence
        kind, channel, opcode, _, seq, payload = frame
        if (kind, channel, opcode) != (4, SERIAL, SERIAL_RX):
            return
        if seq != received_sequence + 1 or not payload or len(pending) + len(payload) > 512:
            raise ValueError("invalid CDC response stream")
        received_sequence = seq
        pending.extend(payload)

    name = select_port(uid, (0xCAFE, 0x4001))
    with serial.Serial(name, 115200, timeout=0.025, write_timeout=1) as port:
        port.reset_input_buffer()
        client = Client(port, None, unsolicited=consume)
        client.hello(uid)
        client.send(0, 6, struct.pack("<B3xQ", SERIAL, 512))
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            state = client.send(SERIAL, SERIAL_STATUS)
            if state[:4] == b"\x01\x00\x00\x08" and struct.unpack_from("<I", state, 4)[0] == 57600:
                break
            time.sleep(0.03)
        else:
            raise TimeoutError("target CDC was not ready")
        for name in (b"VERSION", b"UNSUPPORTED", b"VERSION"):
            token = secrets.token_hex(8).encode()
            sent = b"MKB1 " + token + b" " + name + b"\n"
            prefix = b"MKB1 " + token + b" "
            ack = client.send(SERIAL, SERIAL_WRITE, sent)
            if ack != struct.pack("<H", len(sent)):
                raise ValueError("short management command")
            end = time.monotonic() + 4
            while b"\n" not in pending:
                if time.monotonic() > end:
                    raise TimeoutError(f"waiting for {name!r}")
                client.send(0, 2, b"agent-rx")
            observed, _, remainder = pending.partition(b"\n")
            pending[:] = remainder
            if not observed.startswith(prefix):
                raise ValueError("target reply did not match the request token")
            response = observed[len(prefix):]
            if name == b"VERSION":
                version = response[3:]
                if not response.startswith(b"OK ") or not PYTHON_VERSION.fullmatch(version):
                    raise ValueError("target did not return a Python version")
                if expected_version is not None and version != expected_version:
                    raise ValueError("target Python version differed from expected")
                expected_version = version
            elif response != b"ERR unsupported":
                raise ValueError("target did not reject an unsupported command")
            print(f"{name.decode()}: {response.decode()} verified", flush=True)
        done = b"MKB1 " + secrets.token_hex(8).encode() + b" DONE\n"
        if client.send(SERIAL, SERIAL_WRITE, done) != struct.pack("<H", len(done)):
            raise ValueError("target completion marker was not queued")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("role", choices=("target", "management", "local"))
    parser.add_argument("--uid", required=True)
    parser.add_argument("--target-pid", type=lambda arg: int(arg, 0),
                        choices=(0x4021, 0x4023), default=0x4021)
    parser.add_argument("--expected-version", help='for example "Python 3.12.2"')
    args = parser.parse_args()
    uid = args.uid.upper()
    if not re.fullmatch(r"[0-9A-F]{24}", uid):
        parser.error("expected a 24-digit MCU UID")
    expected = args.expected_version.encode("ascii") if args.expected_version else None
    if expected is not None and not PYTHON_VERSION.fullmatch(expected):
        parser.error("expected version format: Python MAJOR.MINOR.PATCH")

    if args.role == "target":
        target(uid, args.target_pid)
    elif args.role == "management":
        management(uid, expected)
    else:
        process = subprocess.Popen(
            [sys.executable, "-B", str(Path(__file__).resolve()), "target", "--uid", uid,
             "--target-pid", hex(args.target_pid)],
            stdin=subprocess.DEVNULL,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        try:
            management(uid, expected or version_command())
            if process.wait(timeout=6) != 0:
                raise RuntimeError("target process failed")
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=3)


if __name__ == "__main__":
    main()
