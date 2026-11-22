"""Exercise live firmware wire handlers, storage worker and FatFs with an in-memory card."""

import ctypes as c
from io import BytesIO
import struct
from pathlib import Path
import tempfile
import sys
import uuid
import zlib

from raw_disk import Disk, CREATE, OPEN, WRITE, CREATE_JOB

from card_profile import (CARD_INIT, CARD_FORMAT, FORMAT_JOB, USER_BEGIN, USER_COMMIT, USER_DATA,
                          card_info, initialize, read_user, write_user)

from prepare_optical_image import inspect_image, upload_chunks
from probe_management import Client, STATUS, decode, request
from test_prepare_optical_image import image_bytes
from upload_optical_image import (
    BEGIN, DATA, COMMIT, IMAGE_INFO, JOB_CANCEL, JOB_STATUS, SYSTEM, STORAGE, Upload,
)

UID = "0" * 24


class Port:
    def __init__(self, library):
        self.lib = c.CDLL(library)
        self.lib.TestRaw_Validate.argtypes = [c.c_bool, c.c_bool, c.c_uint32, c.c_uint16, c.c_uint32]
        self.lib.TestRaw_Validate.restype = c.c_bool
        self.lib.TestRaw_Transfer.argtypes = [c.c_bool, c.c_bool, c.c_uint32, c.c_void_p, c.c_uint32]
        self.lib.TestRaw_Transfer.restype = c.c_int32
        self.lib.target_tud_msc_get_maxlun_cb.restype = c.c_uint8
        self.lib.TestOptical_Reply.restype = c.c_void_p
        self.lib.TestOptical_Done.restype = c.c_int32
        self.lib.target_tud_msc_is_writable_cb.argtypes = [c.c_uint8]
        self.lib.target_tud_msc_is_writable_cb.restype = c.c_bool
        self.lib.target_tud_msc_write10_cb.argtypes = [c.c_uint8, c.c_uint32, c.c_uint32, c.c_void_p, c.c_uint32]
        self.lib.target_tud_msc_write10_cb.restype = c.c_int32
        self.lib.TestOptical_Sense.restype = c.c_uint8
        self.lib.TestOptical_Eject.restype = c.c_bool
        self.lib.TestOptical_Read.argtypes = [c.c_uint32, c.c_uint16, c.c_uint32]
        self.lib.TestOptical_Read.restype = c.c_int32
        self.lib.TestOptical_Command.argtypes = [c.c_char_p, c.c_void_p, c.c_uint16]
        self.lib.TestOptical_Command.restype = c.c_int32
        self.lib.target_tud_descriptor_configuration_cb.argtypes = [c.c_uint8]
        self.lib.target_tud_descriptor_configuration_cb.restype = c.c_void_p
        self.lib.TestStorage_Work.restype = c.c_bool
        self.lib.TestStorage_SerialEnabled.restype = c.c_bool
        self.lib.TestStorage_Present.argtypes = [c.c_bool]
        self.lib.Management_Input.argtypes = [c.c_uint8]
        self.lib.Management_Output.argtypes = [c.POINTER(c.c_size_t)]
        self.lib.Management_Output.restype = c.c_void_p
        self.lib.Management_OutputConsumed.argtypes = [c.c_size_t]
        self.lib.Management_Tick.argtypes = [c.c_uint64]
        self.lib.TestStorage_Init()
        self.pending = bytearray()
        self.auto_run = True
        self.drop_data_ack = False
        self.now = 0

    @property
    def in_waiting(self):
        return len(self.pending)

    def collect(self):
        size = c.c_size_t()
        reply = self.lib.Management_Output(c.byref(size))
        if size.value:
            frame = c.string_at(reply, size.value)
            self.lib.Management_OutputConsumed(size.value)
            if self.drop_data_ack and decode(frame[:-1])[:3] == (2, STORAGE, DATA):
                self.drop_data_ack = False
            else:
                self.pending.extend(frame)

    def write(self, data):
        for byte in data:
            self.lib.Management_Input(byte)
            self.collect()
        if self.auto_run:
            self.run()
        return len(data)

    def read(self, length):
        data = bytes(self.pending[:length])
        del self.pending[:length]
        return data

    def run(self):
        for _ in range(10):
            if not self.lib.TestStorage_Work():
                break
        else:
            raise AssertionError("storage worker did not become idle")
        self.now += 1
        self.lib.Management_Tick(self.now)
        for _ in range(16):
            before = self.in_waiting
            self.lib.Management_Poll()
            self.collect()
            if self.in_waiting == before:
                break

    def expire(self):
        self.now += 2001
        self.lib.Management_Tick(self.now)
        self.run()


def read_status(client):
    return client.send(SYSTEM, STATUS)


def verify_optical(client, port, data, image_id):
    lib = port.lib
    config = lib.target_tud_descriptor_configuration_cb(0)
    size = int.from_bytes(c.string_at(config + 2, 2), "little")
    descriptor = c.string_at(config, size)
    assert descriptor[4] == 5
    assert bytes([9, 4, 3, 0, 1, 2, 0x0D, 0, 6]) in descriptor
    assert bytes([7, 5, 0x87, 2, 64, 0, 0]) in descriptor
    assert bytes([9, 4, 2, 0, 2, 8, 6, 0x50, 5]) in descriptor
    assert bytes([7, 5, 0x85, 2, 64, 0, 0]) in descriptor
    assert lib.TestOptical_Read(0, 1, 2048) == -1 and lib.TestOptical_Sense() == 0x3A
    upload = Upload(client)
    upload.job, = struct.unpack("<I", client.send(STORAGE, 0x0306, struct.pack("<I", image_id)))
    assert upload.wait_job(3, 3) == image_id
    assert read_status(client)[13] == 7
    assert client.send(STORAGE, IMAGE_INFO)[13] == 1
    assert lib.TestOptical_Read(0, 1, 2048) == -1 and lib.TestOptical_Sense() == 0x28
    assert lib.TestOptical_Read(0, 1, 512) == -1 and lib.TestOptical_Sense() == 0x24
    assert lib.TestOptical_Read(len(data) // 2048 - 1, 2, 4096) == -1
    assert lib.TestOptical_Sense() == 0x21

    disk = Disk(client)
    with disk.attached(0x1234567, False):
        assert lib.TestOptical_Read(0, 1, 2048) == -2
        port.run()
        assert c.string_at(lib.TestOptical_Reply(), 2048) == data[:2048]
        assert disk.read(0) == bytes(512)

    assert lib.TestOptical_Read(0, 2, 4096) == -2
    lib.TestOptical_BotReset()
    port.run()
    assert lib.TestOptical_Done() == 0
    for pos in range(0, len(data), 8192):
        length = min(8192, len(data) - pos)
        assert lib.TestOptical_Read(pos // 2048, length // 2048, length) == -2
        port.run()
        assert lib.TestOptical_Done() == length
        assert c.string_at(lib.TestOptical_Reply(), length) == data[pos:pos + length]

    port.expire()
    client.hello(UID)
    assert client.send(STORAGE, IMAGE_INFO)[13] == 1
    response = c.create_string_buffer(64)
    assert not lib.target_tud_msc_is_writable_cb(0)
    assert lib.target_tud_msc_write10_cb(0, 0, 0, response, 64) == -1
    assert lib.TestOptical_Sense() == 0x27
    cmd = bytearray(16)
    cmd[0], cmd[8] = 0x46, 64
    assert lib.TestOptical_Command(bytes(cmd), response, 64) >= 16
    assert response.raw[6:8] == b"\x00\x08"
    cmd[0] = 0x43
    assert lib.TestOptical_Command(bytes(cmd), response, 64) == 20
    assert response.raw[14] == 0xAA
    assert int.from_bytes(response.raw[16:20], "big") == len(data) // 2048

    id_payload = struct.pack("<I", image_id)
    upload.job, = struct.unpack("<I", client.send(STORAGE, 0x0307, id_payload))
    assert upload.wait_job(4, 3) == image_id
    assert client.send(STORAGE, IMAGE_INFO)[13] == 0
    upload.job, = struct.unpack("<I", client.send(STORAGE, 0x0306, id_payload))
    assert upload.wait_job(3, 3) == image_id
    assert lib.TestOptical_Read(0, 1, 2048) == -1 and lib.TestOptical_Sense() == 0x28
    assert lib.TestOptical_Read(0, 1, 2048) == -2
    assert lib.TestOptical_Eject()
    port.run()
    assert lib.TestOptical_Done() == -1
    assert read_status(client)[13] == 5
    assert client.send(STORAGE, IMAGE_INFO)[13] == 0


def verify_card_profile(client, port):
    uid = "703129" + "00" * 9
    profile = card_info(client)
    assert profile["card_present"] and profile["board_id"] is not None
    assert profile["controller_uid"] == uid and not profile["user_defined"]
    board = uuid.UUID(profile["board_id"])
    assert profile["board_id"] == str(board) and profile["registered_controller"] == uid
    value = bytes(range(256)) * 16
    write_user(client, value)
    assert read_user(client) == value
    assert client.exchange(STORAGE, USER_BEGIN, struct.pack("<H", 4097))[0] == 10

    begin = client.send(STORAGE, USER_BEGIN, struct.pack("<H", 512))
    client.send(STORAGE, USER_DATA, begin + b"\0\0draft")
    assert read_user(client) == value
    assert client.exchange(STORAGE, USER_COMMIT, begin)[0] == 3
    port.expire()
    client.hello(UID)
    assert read_user(client) == value

    port.auto_run = False
    begin = client.send(STORAGE, USER_BEGIN, struct.pack("<H", 4))
    client.send(STORAGE, USER_DATA, begin + b"\0\0stop")
    job = client.send(STORAGE, USER_COMMIT, begin)
    client.send(SYSTEM, JOB_CANCEL, job)
    port.run()
    assert client.send(SYSTEM, JOB_STATUS, job)[4] == 3
    assert read_user(client) == value
    port.auto_run = True

    revision = card_info(client)["user_revision"]
    wrong = client.send(STORAGE, CARD_INIT, uuid.uuid4().bytes)
    assert client.send(SYSTEM, JOB_STATUS, wrong)[4] == 2
    assert card_info(client)["board_id"] == str(board)

    port.lib.TestStorage_Present(False)
    assert not card_info(client)["card_present"]
    port.lib.TestStorage_Present(True)
    assert card_info(client)["user_revision"] != revision
    assert read_user(client) == value


def verify_catalog_and_visibility(client, port):
    caps = client.send(STORAGE, 0x0319)
    assert len(caps) == 8 and int.from_bytes(caps[:4], 'little') == 7
    disk = Disk(client)
    ids = set()
    cursor = 0
    while True:
        job, _ = disk.run(0x031B, 15, struct.pack('<I', cursor))
        page = client.send(STORAGE, 0x031C, struct.pack('<I', job))
        assert len(page) == 8 + page[4] * 12 and page[4] <= 4
        for offset in range(8, len(page), 12):
            image, sectors, fmt = struct.unpack_from('<IIB', page, offset)
            assert image not in ids and sectors == 4 and fmt in (2, 3)
            ids.add(image)
        cursor, = struct.unpack_from('<I', page)
        if not cursor: break
    assert ids == {0x1234567, 0x1234568}
    with disk.attached(0x1234567, False):
        assert client.exchange(STORAGE, 0x031D, struct.pack('<I', 0x1234568))[0] == 3
        assert client.exchange(STORAGE, 0x031A, bytes([0]))[0] == 3
    disk.run(0x031D, 16, struct.pack('<I', 0x1234568))
    missing = client.send(STORAGE, OPEN, struct.pack('<IB3x', 0x1234568, False))
    terminal = client.send(SYSTEM, JOB_STATUS, missing)
    assert terminal[4] == 2 and struct.unpack_from('<H', terminal, 6)[0] == 17
    # The open ACK is only a job receipt; its terminal result must be NOT_FOUND.
    # Close any failed-open state before checking LUN visibility.
    client.send(STORAGE, 0x031A, bytes([0]))
    port.lib.TargetMsc_SetOpticalVisible.argtypes = [c.c_bool]
    port.lib.StorageService_OpticalApplied.argtypes = [c.c_bool]
    port.lib.TargetMsc_SetOpticalVisible(False)
    port.lib.StorageService_OpticalApplied(False)
    assert port.lib.target_tud_msc_get_maxlun_cb() == 1
    inquiry = c.create_string_buffer(96)
    port.lib.target_tud_msc_inquiry2_cb.argtypes = [c.c_uint8, c.c_void_p, c.c_uint32]
    assert port.lib.target_tud_msc_inquiry2_cb(0, inquiry, len(inquiry)) > 0
    assert inquiry.raw[0] & 0x1f == 0, 'LUN0 must be a disk, never a CD/DVD when optical is OFF'
    assert client.send(STORAGE, 0x0319)[4:6] == bytes([0, 0])
    client.send(STORAGE, 0x031A, bytes([1]))
    port.lib.TargetMsc_SetOpticalVisible(True)
    port.lib.StorageService_OpticalApplied(True)
    assert port.lib.target_tud_msc_get_maxlun_cb() == 2


def verify_writable_disk(client, port):
    disk = Disk(client)
    disk_id = 0x1234567
    seed = bytes(512) + bytes(range(256)) * 2 + bytes(1024)
    checksum = zlib.crc32(seed)
    disk.create(disk_id, 2, BytesIO(seed), len(seed), checksum)
    with disk.attached(disk_id, False):
        assert disk.info() == (disk_id, 4, 2, 3, False)
        assert b"".join(disk.read(sector) for sector in range(4)) == seed
        args = struct.pack("<II", disk.lease, 0) + bytes(512)
        assert client.exchange(STORAGE, WRITE, args)[0] == 16
        args = struct.pack("<II", disk.lease, 4) + bytes(512)
        assert client.exchange(STORAGE, WRITE, args)[0] == 16

    replacement = bytes(range(255, -1, -1)) * 2
    with disk.attached(disk_id, True):
        args = struct.pack("<II", disk.lease, 4) + bytes(512)
        assert client.exchange(STORAGE, WRITE, args)[0] == 10
        disk.write(1, replacement)
        disk.sync()
        assert disk.read(1) == replacement
    with disk.attached(disk_id, False):
        assert disk.read(0) == bytes(512)
        assert disk.read(1) == replacement

    with tempfile.TemporaryDirectory() as folder:
        before = Path(folder) / "before.img"
        after = Path(folder) / "after.img"
        before.write_bytes(seed)
        changed = bytes([0xA5]) * 512
        after.write_bytes(changed + seed[512:1024] + changed + seed[1536:])
        with disk.attached(disk_id, True):
            assert disk.apply(before, after, len(seed)) == 2
            assert disk.apply(before, after, len(seed)) == 0
        after.write_bytes(seed[:512] + changed + seed[1024:])
        with disk.attached(disk_id, True):
            try:
                disk.apply(before, after, len(seed))
            except ValueError as exc:
                assert "sector 1 differs" in str(exc)
            else:
                raise AssertionError("patch accepted a conflicting base sector")
            assert disk.read(1) == replacement
            disk.write(0, bytes(512))
            disk.write(2, bytes(512))

    lib = port.lib
    assert lib.target_tud_msc_get_maxlun_cb() == 2
    disk.mount_usb(disk_id, "management", True)
    assert disk.info() == (disk_id, 4, 2, 5, True)
    assert client.exchange(STORAGE, CARD_FORMAT, struct.pack("<B3x", 2))[0] == 3
    assert not lib.TestRaw_Validate(True, False, 0, 1, 512)
    assert lib.TestOptical_Sense() == 0x3A
    assert not lib.TestRaw_Validate(False, True, 2, 2, 512)
    assert lib.TestOptical_Sense() == 0x24
    assert not lib.TestRaw_Validate(False, True, 2, 2, 1024)
    assert lib.TestOptical_Sense() == 0x28
    assert lib.TestRaw_Validate(False, True, 2, 2, 1024)
    marker = bytes([0xA3]) * 1024
    write_buffer = c.create_string_buffer(marker)
    assert lib.TestRaw_Transfer(False, True, 2, write_buffer, 1024) == -2
    assert lib.TestOptical_Done() == 0
    c.memset(write_buffer, 0, 1024)
    port.run()
    assert lib.TestOptical_Done() == 1024
    port.expire()
    client.hello(UID)
    assert disk.active()[3] == 5
    assert client.exchange(STORAGE, OPEN, struct.pack("<IBB2x", disk_id, False, 1))[0] == 3
    disk.unmount_usb(disk_id)
    disk.mount_usb(disk_id, "target", False)
    assert disk.info() == (disk_id, 4, 2, 4, False)
    assert not lib.TestRaw_Validate(True, False, 2, 2, 1024)
    assert lib.TestOptical_Sense() == 0x28
    assert not lib.TestRaw_Validate(True, True, 2, 2, 1024)
    assert lib.TestOptical_Sense() == 0x27
    assert not lib.TestRaw_Validate(True, False, 3, 2, 1024)
    assert lib.TestOptical_Sense() == 0x21
    assert lib.TestRaw_Validate(True, False, 2, 2, 1024)
    read_buffer = c.create_string_buffer(1024)
    assert lib.TestRaw_Transfer(True, False, 2, read_buffer, 1024) == -2
    lib.TestOptical_BotReset()
    port.run()
    assert lib.TestOptical_Done() == 0 and read_buffer.raw == bytes(1024)
    assert lib.TestRaw_Validate(True, False, 2, 2, 1024)
    assert lib.TestRaw_Transfer(True, False, 2, read_buffer, 1024) == -2
    port.run()
    assert lib.TestOptical_Done() == 1024 and read_buffer.raw == marker
    disk.unmount_usb(disk_id)
    with disk.attached(disk_id, True):
        assert disk.read(2) == marker[:512]
        disk.write(2, bytes(512))
        disk.write(3, bytes(512))

    raw = struct.pack("<IIB3x", disk_id, 4, 2)
    duplicate = client.send(STORAGE, CREATE, raw)
    failed = client.send(SYSTEM, JOB_STATUS, duplicate)
    assert failed[4] == 2 and int.from_bytes(failed[6:8], "little") == 3

    try:
        disk.create(0x1234568, 3, BytesIO(seed), len(seed), checksum ^ 1)
    except RuntimeError as exc:
        assert "status 12" in str(exc)
    else:
        raise AssertionError("bad CRC published an image")
    missing = client.send(STORAGE, OPEN, struct.pack("<IB3x", 0x1234568, False))
    assert int.from_bytes(client.send(SYSTEM, JOB_STATUS, missing)[6:8], "little") == 17

    _, disk.lease = disk.run(CREATE, CREATE_JOB, struct.pack("<IIB3x", 0x1234568, 4, 3))
    disk.write(0, replacement)
    port.expire()
    client.hello(UID)
    missing = client.send(STORAGE, OPEN, struct.pack("<IB3x", 0x1234568, False))
    assert int.from_bytes(client.send(SYSTEM, JOB_STATUS, missing)[6:8], "little") == 17
    disk.create(0x1234568, 3, BytesIO(seed), len(seed), checksum)
    with disk.attached(0x1234568, False):
        assert disk.read(0) == bytes(512)
    with disk.attached(disk_id, False):
        assert disk.read(1) == replacement


def verify_card_format(client, port, image):
    exfat = b"\x02\0\0\0"
    assert client.exchange(STORAGE, CARD_FORMAT, b"\x02\x01\0\0")[0] == 2
    port.lib.TestStorage_Present(False)
    assert client.exchange(STORAGE, CARD_FORMAT, exfat)[0] == 9
    port.lib.TestStorage_Present(True)

    stream = BytesIO(image_bytes())
    image_id = Upload(client).send_image(stream, image, "before-format.iso", 3)
    mount = client.send(STORAGE, 0x0306, struct.pack("<I", image_id))
    assert client.send(SYSTEM, JOB_STATUS, mount)[4] == 1
    assert client.exchange(STORAGE, CARD_FORMAT, exfat)[0] == 3
    unmount = client.send(STORAGE, 0x0307, struct.pack("<I", image_id))
    assert client.send(SYSTEM, JOB_STATUS, unmount)[4] == 1
    assert card_info(client)["board_id"] is not None and card_info(client)["user_defined"]

    port.auto_run = False
    job = client.send(STORAGE, CARD_FORMAT, exfat)
    assert read_status(client)[13] == 10
    assert not port.lib.TestStorage_SerialEnabled()
    assert client.exchange(STORAGE, BEGIN, image.begin_payload("not-while-formatting.iso"))[0] == 3
    assert client.exchange(SYSTEM, JOB_CANCEL, job)[0] == 3
    port.expire()
    client.hello(UID)
    port.run()
    result = client.send(SYSTEM, JOB_STATUS, job)
    assert tuple(result[4:8]) == (1, FORMAT_JOB, 0, 0)
    assert read_status(client)[13] == 1 and port.lib.TestStorage_SerialEnabled()
    assert client.exchange(STORAGE, IMAGE_INFO)[0] == 17
    info = card_info(client)
    assert info["card_present"] and not info["user_defined"] and info["board_id"] is not None
    assert info["profile_status"] == 0 and info["user_status"] == 17
    assert client.send(STORAGE, 0x031E) == bytes(4)
    port.auto_run = True

    stream.seek(0)
    assert Upload(client).send_image(stream, image, "new-exfat.iso", 3)
    port.lib.TestStorage_Present(False)
    port.lib.TestStorage_Present(True)
    assert read_status(client)[13] == 5
    job = client.send(STORAGE, CARD_FORMAT, struct.pack("<B3x", 1))
    assert tuple(client.send(SYSTEM, JOB_STATUS, job)[4:8]) == (1, FORMAT_JOB, 0, 0)
    assert read_status(client)[13] == 1


def test(library):
    events = []
    port = Port(library)
    client = Client(port, None, unsolicited=events.append)
    try:
        hello = client.hello(UID)
        assert struct.unpack_from("<I", hello, 28)[0] & 0x14 == 0x14
        assert read_status(client)[13] == 1
        verify_card_profile(client, port)
        assert client.send(STORAGE, 0x031E) == bytes(4)
        name = "서버 A".encode("utf-8")
        rename = Upload(client)
        rename.job = int.from_bytes(client.send(STORAGE, 0x031F, struct.pack("<H", len(name)) + name), "little")
        rename.wait_job(17, 10)
        assert client.send(STORAGE, 0x031E) == struct.pack("<H2x", len(name)) + name
        assert client.exchange(STORAGE, 0x031F, bytes([1, 0, 0xff]))[0] == 2
        port.lib.TestStorage_Present(False)
        assert client.exchange(STORAGE, 0x031E)[0] == 9
        port.lib.TestStorage_Present(True)
        assert client.send(STORAGE, 0x031E) == struct.pack("<H2x", len(name)) + name
        verify_writable_disk(client, port)
        verify_catalog_and_visibility(client, port)
        data = image_bytes()
        stream = BytesIO(data)
        image = inspect_image(stream)
        port.drop_data_ack = True
        image_id = Upload(client, data_window=4).send_image(stream, image, "test.iso", 3)
        assert not port.drop_data_ack
        assert read_status(client)[13] == 5
        assert events and any(event[2] == 0x0081 for event in events)
        verify_optical(client, port, data, image_id)
        port.lib.TestStorage_Present(False)
        assert read_status(client)[13] == 0
        assert client.exchange(STORAGE, IMAGE_INFO)[0] == 17
        port.lib.TestStorage_Present(True)
        assert read_status(client)[13] == 5
        assert int.from_bytes(client.send(STORAGE, IMAGE_INFO)[:4], "little") != image_id

        port.auto_run = False
        job = client.send(STORAGE, BEGIN, image.begin_payload("test.iso"))
        assert read_status(client)[13] == 2
        assert not port.lib.TestStorage_SerialEnabled()
        assert client.exchange(2, 0x0201, b"test")[0] == 3
        port.run()
        assert read_status(client)[13] == 3
        upload = int.from_bytes(client.send(SYSTEM, JOB_STATUS, job)[12:16], "little")
        chunks = iter(upload_chunks(stream, image, upload))
        first = next(chunks)
        seq = client.sequence[STORAGE]
        reply = client.send(STORAGE, DATA, first)
        port.write(request(STORAGE, DATA, client.session, seq, first))
        assert client.wait_reply(STORAGE, DATA, seq)[3] == reply
        full_chunks = (64 * 1024) // 1008
        for i in range(2, full_chunks + 1):
            assert int.from_bytes(client.send(STORAGE, DATA, next(chunks)), "little") == i * 1008
        pending = next(chunks)
        assert client.exchange(STORAGE, DATA, pending)[:2] == (4, 10)
        assert read_status(client)[40:48] == struct.pack("<Q", full_chunks * 1008)
        port.run()
        assert int.from_bytes(client.send(STORAGE, DATA, pending), "little") == (full_chunks + 1) * 1008
        assert client.exchange(STORAGE, COMMIT, struct.pack("<I", upload))[0] == 3
        port.expire()
        port.auto_run = True
        client.hello(UID)
        assert read_status(client)[13] == 1
        assert client.exchange(STORAGE, IMAGE_INFO)[0] == 17
        assert port.lib.TestStorage_SerialEnabled()

        port.auto_run = False
        job = client.send(STORAGE, BEGIN, image.begin_payload("cancel.iso"))
        client.send(SYSTEM, JOB_CANCEL, job)
        port.run()
        assert client.send(SYSTEM, JOB_STATUS, job)[4] == 3
        assert read_status(client)[13] == 1
        port.auto_run = True
        verify_card_format(client, port, image)
    finally:
        port.lib.TestStorage_Free()


if __name__ == "__main__":
    test(sys.argv[1])
    print("Management upload, job events, replay, queue pressure, cancel, lease, management/target writable MSC, card format and optical read/eject tests passed")
