"""Exercise the MiniKMB management-to-target CDC bridge without HID."""

import argparse
from contextlib import contextmanager
import json
from pathlib import Path
import secrets
import struct
import sys
from time import monotonic, sleep

import serial

from probe_management import Client, request, select_port

SERIAL = 2
WRITE, STATUS, STATS, RX = 0x0201, 0x0202, 0x0203, 0x0280
PING, CREDIT = 2, 6


@contextmanager
def observe_failure(client, terminal, path, metadata, progress):
    """Best-effort read-only diagnostics before the target CDC context closes.

    These are sequential observations after detection, not an atomic USB fault
    snapshot. Do not retry the failed data operation or hide its exception.
    """
    try:
        yield
    except Exception as error:
        if path is not None:
            report = {**metadata, 'schema_version':1, 'stage':'failure-before-port-close',
                      'error':f'{type(error).__name__}: {error}',
                      'progress':dict(progress), 'observed_monotonic':monotonic(),
                      'atomic_snapshot':False, 'session':client.session, 'queries':{}}
            try:
                report['target_port_open'] = terminal.is_open
                report['target_dtr_requested'] = terminal.dtr
            except Exception as diagnostic_error:
                report['target_port_error'] = str(diagnostic_error)
            for name, opcode, size in (('serial_status', STATUS, 16), ('serial_stats', STATS, 8)):
                observation = {'opcode':opcode, 'started_monotonic':monotonic()}
                report['queries'][name] = observation
                try:
                    status, detail, payload = client.exchange(SERIAL, opcode, attempts=1, timeout=.15)
                    observation.update(status=status, detail=detail, payload_hex=payload.hex())
                    if status == 0 and len(payload) == size:
                        if opcode == STATUS:
                            values = struct.unpack('<4B3I', payload)
                            observation['decoded'] = dict(zip(
                                ('open', 'stop_bits', 'parity', 'data_bits', 'baud', 'tx_queued', 'rx_queued'), values))
                        else:
                            observation['decoded'] = dict(zip(
                                ('tx_discarded', 'rx_discarded'), struct.unpack('<II', payload)))
                    elif status == 0:
                        observation['decode_error'] = f'expected {size} bytes, received {len(payload)}'
                except Exception as diagnostic_error:
                    observation['error'] = f'{type(diagnostic_error).__name__}: {diagnostic_error}'
                observation['finished_monotonic'] = monotonic()
            try:
                with Path(path).open('x', encoding='utf-8') as output:
                    json.dump(report, output, indent=2)
                    output.write('\n')
            except Exception as diagnostic_error:
                print(f'Could not save serial failure observation: {diagnostic_error}', file=sys.stderr)
        raise


def exercise(uid, byte_count, target_pid=0x4011, failure_report=None):
    if failure_report is not None and Path(failure_report).exists():
        raise FileExistsError('failure report already exists; choose a new path')
    incoming = bytearray()
    sequence = 0
    progress = {'phase':'opening-target', 'offset':None}

    def consume(frame):
        nonlocal sequence
        kind, channel, opcode, _, seq, payload = frame
        if (kind, channel, opcode) != (4, SERIAL, RX):
            return
        if seq != sequence + 1 or not payload or len(payload) > 256:
            raise ValueError("invalid SERIAL_RX sequence or size")
        sequence = seq
        incoming.extend(payload)

    management = select_port(uid, (0xCAFE, 0x4001))
    target = select_port(uid, (0xCAFE, target_pid))
    with serial.Serial(management, 115200, timeout=0.03, write_timeout=1) as mgmt:
        mgmt.reset_input_buffer()
        client = Client(mgmt, None, unsolicited=consume)
        info = client.hello(uid)
        if not (struct.unpack_from("<I", info, 28)[0] & 2):
            raise RuntimeError("selected MCU does not advertise SERIAL")
        def serial_write_wire(payload):
            seq = client.sequence[SERIAL]
            return seq, request(SERIAL, WRITE, client.session, seq, payload)

        expected_refusal, wire = serial_write_wire(b"closed CDC test")
        mgmt.write(wire)
        status, detail, _, result = client.wait_reply(SERIAL, WRITE, expected_refusal)
        if (status, detail, result) != (9, 3, b""):
            raise ValueError("a closed target CDC did not refuse data")
        client.sequence[SERIAL] += 1

        with serial.Serial(target, 57600, timeout=1, write_timeout=1) as terminal, observe_failure(
                client, terminal, failure_report,
                {'uid':uid, 'target_pid':target_pid, 'management_port':management,
                 'target_port':target, 'requested_bytes_each_direction':byte_count}, progress):
            terminal.reset_input_buffer()
            until = monotonic() + 2
            while monotonic() < until:
                state = client.send(SERIAL, STATUS)
                if state[:4] == b"\x01\x00\x00\x08" and struct.unpack_from("<I", state, 4)[0] == 57600:
                    break
                sleep(0.02)
            else:
                raise RuntimeError("target CDC DTR or line coding not established")

            data = b"\x00single target transfer\xff"
            progress['phase'] = 'replay-check'
            seq, duplicate = serial_write_wire(data)
            if client.send(SERIAL, WRITE, data) != struct.pack("<H", len(data)):
                raise ValueError("first write did not receive the expected ACK")
            mgmt.write(duplicate)
            status, detail, _, result = client.wait_reply(SERIAL, WRITE, seq)
            if (status, detail, result) != (0, 0, struct.pack("<H", len(data))):
                raise ValueError("SERIAL_WRITE replay did not return the cached response")
            if terminal.read(len(data)) != data:
                raise ValueError("SERIAL_WRITE data did not arrive")
            terminal.timeout = 0.10
            if terminal.read(1):
                raise ValueError("SERIAL_WRITE replay duplicated target data")
            terminal.timeout = 1

            # Hold credit at zero while the target sends its first block.
            withheld = secrets.token_bytes(256)
            progress['phase'] = 'withheld-credit-check'
            if terminal.write(withheld) != len(withheld):
                raise IOError("short target write before credit")
            until = monotonic() + 2
            while monotonic() < until:
                state = client.send(SERIAL, STATUS)
                if struct.unpack_from("<I", state, 12)[0] >= len(withheld):
                    break
                sleep(0.02)
            else:
                raise TimeoutError("target data not retained before credit")
            if incoming:
                raise ValueError("target data sent without credit")

            def grant(limit):
                client.send(0, CREDIT, struct.pack("<B3xQ", SERIAL, limit))

            def wait_stream(length):
                end = monotonic() + 2
                while len(incoming) < length:
                    if monotonic() >= end:
                        raise TimeoutError("waiting for SERIAL_RX")
                    client.send(0, PING, b"rx-check")
                data = bytes(incoming[:length])
                del incoming[:length]
                return data

            grant(4096)
            if wait_stream(len(withheld)) != withheld:
                raise ValueError("incorrect retained data")
            sent_to_management = len(withheld)
            next_credit = 4096
            started = monotonic()
            for offset in range(0, byte_count, 256):
                size = min(256, byte_count - offset)
                progress.clear()
                progress.update(phase='target-write', offset=offset, size=size)
                from_mgmt = secrets.token_bytes(size)
                from_target = secrets.token_bytes(size)
                if terminal.write(from_target) != size:
                    raise IOError(f"short target write at offset {offset}")
                progress['phase'] = 'management-write-ack'
                reply = client.send(SERIAL, WRITE, from_mgmt)
                expected_ack = struct.pack("<H", size)
                if reply != expected_ack:
                    raise ValueError(
                        f"SERIAL_WRITE ACK mismatch at offset {offset}: "
                        f"expected {expected_ack.hex()}, received {reply.hex()}"
                    )
                progress['phase'] = 'target-read'
                received = terminal.read(size)
                progress.update(received_bytes=len(received), received_prefix=received[:16].hex(),
                                expected_prefix=from_mgmt[:16].hex())
                if len(received) != size:
                    raise ValueError(
                        f"management to target short read at offset {offset}: "
                        f"received {len(received)}/{size} bytes; "
                        f"prefix {received[:16].hex()}"
                    )
                if received != from_mgmt:
                    mismatch = next(index for index, values in enumerate(zip(received, from_mgmt))
                                    if values[0] != values[1])
                    raise ValueError(
                        f"management to target data mismatch at byte {offset + mismatch}: "
                        f"expected {from_mgmt[mismatch]:02x}, received {received[mismatch]:02x}; "
                        f"expected prefix {from_mgmt[:16].hex()}, received prefix {received[:16].hex()}"
                    )
                progress['phase'] = 'management-read'
                if wait_stream(size) != from_target:
                    raise ValueError(f"target to management mismatch at offset {offset}")
                sent_to_management += size
                if next_credit - sent_to_management < 1024:
                    next_credit = sent_to_management + 4096
                    grant(next_credit)

            if incoming:
                raise ValueError("unexpected bytes left over after transfer")
            print(f"{byte_count} bytes each direction matched in {monotonic()-started:.3f} s")
            progress['phase'] = 'dtr-close-check'
            terminal.dtr = False
            until = monotonic() + 2
            while monotonic() < until:
                if not client.send(SERIAL, STATUS)[0]:
                    break
                sleep(0.02)
            else:
                raise TimeoutError("target CDC did not close")
            print("DTR closed. Cumulative discard counts:", struct.unpack("<II", client.send(SERIAL, STATS)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--uid", required=True)
    parser.add_argument("--bytes", type=int, default=65536)
    parser.add_argument("--target-pid", type=lambda value: int(value, 0), default=0x4011)
    parser.add_argument('--failure-report', type=Path,
                        help='new JSON path for read-only observations before closing a failed target CDC')
    args = parser.parse_args()
    uid = args.uid.upper()
    if len(uid) != 24 or args.bytes <= 0:
        parser.error("expected a 24-digit MCU UID and a positive size")
    exercise(uid, args.bytes, args.target_pid, args.failure_report)

