"""Exercise real Client framing against scripted board responses, without USB I/O."""
import struct
import sys
import unittest
from unittest.mock import patch
import zlib

if len(sys.argv) > 1 and not sys.argv[1].startswith('-'):
    sys.path.insert(0, sys.argv.pop(1))
import probe_management as pm

UID = '000000000000000000000001'
SESSION = 0x1122334455667788


class BoardPort:
    def __init__(self, statuses, *, wrong_uid=False, inconsistent_info=False):
        self.statuses = list(statuses)
        self.wrong_uid = wrong_uid
        self.inconsistent_info = inconsistent_info
        self.received = bytearray()
        self.hellos = []
        self.info_requests = 0

    @property
    def in_waiting(self):
        return len(self.received)

    def read(self, size):
        result = bytes(self.received[:size])
        del self.received[:size]
        return result

    def write(self, wire):
        packets = [x for x in wire.split(b'\0') if x]
        if len(packets) != 1:
            raise AssertionError('unexpected number of requests')
        kind, channel, op, session, sequence, payload = pm.decode(packets[0])
        if (kind, channel, sequence) != (1, 0, 1):
            raise AssertionError('unexpected request identity')
        if op == pm.HELLO:
            if session or len(payload) != 12:
                raise AssertionError('malformed HELLO')
            self.hellos.append(bytes(wire))
            status, detail, reply_session = self.statuses.pop(0)
            self.info = bytearray(40)
            self.info[:8] = payload[4:]
            self.info[8:20] = bytes.fromhex('00' * 12 if self.wrong_uid else UID)
            self.info[20:26] = struct.pack('<HHH', 0, 1, 0)
            self.info[36] = 4
            body = b'' if status else bytes(self.info)
        elif op == pm.INFO:
            if session != SESSION or payload:
                raise AssertionError('INFO did not use accepted session')
            self.info_requests += 1
            status, detail, reply_session = 0, 0, session
            body = bytes(self.info)
            if self.inconsistent_info:
                body = body[:-1] + b'\x01'
        else:
            raise AssertionError('test sent a non-handshake command')
        body = struct.pack('<HHI', status, 0, detail) + body
        raw = pm.HEADER.pack(0x4d4b, 1, 2, 0, channel, op, reply_session, sequence, len(body), 0) + body
        self.received.extend(pm.cobs_encode(raw + struct.pack('<I', zlib.crc32(raw))) + b'\0')
        return len(wire)


class HelloTests(unittest.TestCase):
    def test_transient_not_ready_replays_identical_nonce_and_verifies_info(self):
        port = BoardPort([(9, 0, 0), (0, 0, SESSION)])
        client = pm.Client(port, None)
        with patch.object(pm.time, 'sleep') as sleep:
            result = client.hello(UID)
        self.assertEqual(port.hellos[0], port.hellos[1])
        self.assertEqual(len(result), 40)
        self.assertEqual(client.session, SESSION)
        self.assertEqual(port.info_requests, 1)
        sleep.assert_called_once_with(.05)

    def test_persistent_not_ready_stops_at_three(self):
        port = BoardPort([(9, 0, 0)] * 3)
        client = pm.Client(port, None)
        with patch.object(pm.time, 'sleep') as sleep:
            with self.assertRaisesRegex(RuntimeError, 'HELLO failed: 9, 0'):
                client.hello(UID)
        self.assertEqual(len(port.hellos), 3)
        self.assertEqual(len(set(port.hellos)), 1)
        self.assertEqual(sleep.call_count, 2)
        self.assertEqual(client.session, 0)
        self.assertEqual(port.info_requests, 0)

    def test_other_status_is_not_retried(self):
        for status, detail, session in ((1, 0, 0), (9, 4, 0), (9, 0, SESSION)):
            with self.subTest(status=status, detail=detail, session=session):
                port = BoardPort([(status, detail, session)])
                with self.assertRaisesRegex(RuntimeError, 'HELLO failed:'):
                    pm.Client(port, None).hello(UID)
                self.assertEqual(len(port.hellos), 1)

    def test_foreign_uid_is_rejected(self):
        port = BoardPort([(0, 0, SESSION)], wrong_uid=True)
        with self.assertRaisesRegex(ValueError, 'nonce or UID mismatch'):
            pm.Client(port, None).hello(UID)
        self.assertEqual(port.info_requests, 0)

    def test_inconsistent_info_is_rejected(self):
        port = BoardPort([(0, 0, SESSION)], inconsistent_info=True)
        with self.assertRaisesRegex(ValueError, 'INFO and HELLO disagree'):
            pm.Client(port, None).hello(UID)

    def test_normal_connection_does_not_retry(self):
        port = BoardPort([(0, 0, SESSION)])
        with patch.object(pm.time, 'sleep') as sleep:
            pm.Client(port, None).hello(UID)
        self.assertEqual(len(port.hellos), 1)
        sleep.assert_not_called()


if __name__ == '__main__':
    unittest.main(verbosity=2)
