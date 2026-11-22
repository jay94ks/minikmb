"""Failure timing regression for the real serial exercise; no USB device access."""
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

if len(sys.argv) > 1 and not sys.argv[1].startswith('-'):
    location = Path(sys.argv.pop(1))
    sys.path.insert(0, str(location))
else:
    location = Path(__file__).resolve().parent
import probe_serial_bridge as bridge

UID = '000000000000000000000001'


class SerialPort:
    def __init__(self, board, target):
        self.board, self.target = board, target
        self.is_open, self.dtr = False, True
        self.timeout = 1
        self.name = 'TARGET' if target else 'MANAGEMENT'
        self.to_target = bytearray()

    def __enter__(self):
        self.is_open = True
        self.board.events.append(('open', self.name))
        return self

    def __exit__(self, *exc):
        self.board.events.append(('close', self.name))
        self.is_open = False

    def reset_input_buffer(self):
        pass

    def write(self, data):
        if self.target:
            self.board.from_target.extend(data)
        return len(data)

    def read(self, length):
        if self.board.short_read and self.board.main_write:
            return b''
        value = bytes(self.to_target[:length])
        del self.to_target[:length]
        return value


class Board:
    def __init__(self, short_read=True, query_error=False):
        self.events, self.from_target = [], bytearray()
        self.short_read, self.query_error = short_read, query_error
        self.main_write, self.credited, self.rx_seq = False, False, 0
        self.management = SerialPort(self, False)
        self.target = SerialPort(self, True)
        self.exchange_calls = []

    def serial(self, name, *args, **kwargs):
        return self.management if name == 'MANAGEMENT' else self.target

    def client(self, *args, unsolicited=None, **kwargs):
        self.unsolicited = unsolicited
        return self

    session = 123
    sequence = [1] * 8

    def hello(self, uid):
        assert uid == UID
        info = bytearray(40)
        struct.pack_into('<I', info, 28, 2)
        return info

    def wait_reply(self, channel, opcode, seq):
        if not self.target.is_open:
            return 9, 3, self.session, b''
        return 0, 0, self.session, struct.pack('<H', 24)

    def send(self, channel, opcode, data=b''):
        if opcode == bridge.STATUS:
            return struct.pack('<4B3I', int(self.target.dtr), 0, 0, 8, 57600, 0, len(self.from_target))
        if opcode == bridge.WRITE:
            self.main_write = len(data) != 24
            self.target.to_target.extend(data)
            return struct.pack('<H', len(data))
        if opcode == bridge.CREDIT:
            self.credited = True
        if self.credited and self.from_target:
            self.rx_seq += 1
            value = bytes(self.from_target)
            self.from_target.clear()
            self.unsolicited((4, bridge.SERIAL, bridge.RX, self.session, self.rx_seq, value))
        if opcode == bridge.STATS:
            return struct.pack('<II', 0, 0)
        return b''

    def exchange(self, channel, opcode, *, attempts, timeout):
        assert self.target.is_open and self.management.is_open
        assert attempts == 1 and timeout == .15
        self.events.append(('observe', opcode, self.target.dtr))
        self.exchange_calls.append(opcode)
        if self.query_error:
            raise TimeoutError('diagnostic unavailable')
        if opcode == bridge.STATUS:
            return 0, 0, struct.pack('<4B3I', 1, 0, 0, 8, 57600, 256, 256)
        return 0, 0, struct.pack('<II', 2, 3)


class FailureTests(unittest.TestCase):
    def run_exercise(self, board, path):
        with patch.object(bridge.serial, 'Serial', board.serial), \
             patch.object(bridge, 'Client', board.client), \
             patch.object(bridge, 'select_port', lambda uid, pair: 'TARGET' if pair[1] == 0x4025 else 'MANAGEMENT'):
            return bridge.exercise(UID, 256, 0x4025, path)

    def test_short_read_is_observed_before_dtr_or_ports_close(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'failure.json'
            board = Board()
            with self.assertRaisesRegex(ValueError, 'short read at offset 0: received 0/256'):
                self.run_exercise(board, path)
            report = json.loads(path.read_text())
            self.assertEqual(report['progress']['phase'], 'target-read')
            self.assertEqual(report['progress']['offset'], 0)
            self.assertEqual(report['progress']['received_bytes'], 0)
            self.assertEqual(report['uid'], UID)
            self.assertTrue(report['target_port_open'])
            self.assertTrue(report['target_dtr_requested'])
            self.assertFalse(report['atomic_snapshot'])
            self.assertEqual(report['queries']['serial_status']['decoded']['tx_queued'], 256)
            self.assertEqual(report['queries']['serial_stats']['decoded']['rx_discarded'], 3)
            close = board.events.index(('close', 'TARGET'))
            self.assertLess(board.events.index(('observe', bridge.STATUS, True)), close)
            self.assertLess(board.events.index(('observe', bridge.STATS, True)), close)

    def test_diagnostic_timeout_keeps_original_data_error(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'failure.json'
            board = Board(query_error=True)
            with self.assertRaisesRegex(ValueError, 'short read'):
                self.run_exercise(board, path)
            report = json.loads(path.read_text())
            self.assertTrue(all('TimeoutError' in q['error'] for q in report['queries'].values()))
            self.assertEqual(board.exchange_calls, [bridge.STATUS, bridge.STATS])

    def test_no_report_option_preserves_no_extra_diagnostic_commands(self):
        board = Board()
        with self.assertRaisesRegex(ValueError, 'short read'):
            self.run_exercise(board, None)
        self.assertEqual(board.exchange_calls, [])

    def test_existing_report_refuses_before_opening_any_port(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'existing.json'
            path.write_text('keep')
            board = Board()
            with self.assertRaises(FileExistsError):
                self.run_exercise(board, path)
            self.assertEqual(board.events, [])
            self.assertEqual(path.read_text(), 'keep')

    def test_save_error_does_not_replace_original_error(self):
        with tempfile.TemporaryDirectory() as folder:
            board = Board()
            with self.assertRaisesRegex(ValueError, 'short read'), patch.object(bridge.sys, 'stderr'):
                self.run_exercise(board, Path(folder) / 'missing' / 'failure.json')
            self.assertFalse(board.target.is_open)

    def test_bad_status_payload_is_preserved_without_decoding(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'failure.json'
            board = Board()
            with patch.object(board, 'exchange', return_value=(0, 0, b'\x01')):
                with self.assertRaisesRegex(ValueError, 'short read'):
                    self.run_exercise(board, path)
            observation = json.loads(path.read_text())['queries']['serial_status']
            self.assertEqual(observation['payload_hex'], '01')
            self.assertIn('expected 16 bytes', observation['decode_error'])
            self.assertNotIn('decoded', observation)

    def test_device_error_keeps_raw_status_and_original_failure(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'failure.json'
            board = Board()
            with patch.object(board, 'exchange', return_value=(9, 3, b'')):
                with self.assertRaisesRegex(ValueError, 'short read'):
                    self.run_exercise(board, path)
            observation = json.loads(path.read_text())['queries']['serial_status']
            self.assertEqual((observation['status'], observation['detail']), (9, 3))
            self.assertNotIn('decoded', observation)

    def test_success_adds_no_diagnostic_commands_and_no_report(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'not-created.json'
            board = Board(short_read=False)
            self.run_exercise(board, path)
            self.assertEqual(board.exchange_calls, [])
            self.assertFalse(path.exists())


if __name__ == '__main__':
    unittest.main(verbosity=2)
