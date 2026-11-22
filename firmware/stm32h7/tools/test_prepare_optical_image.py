"""Host-only tests for the constrained optical upload profile."""

import zlib
from io import BytesIO
import struct
import unittest

from prepare_optical_image import BLOCK, CHUNK, ImageError, inspect_image, upload_chunks


def image_bytes(*, udf=False, bios=True, efi=False):
    blocks = 300 if udf else 40
    image = bytearray(blocks * BLOCK)

    def descriptor(lba, descriptor_id, kind=0):
        image[lba * BLOCK:lba * BLOCK + 7] = bytes([kind]) + descriptor_id + b"\x01"

    if udf:
        descriptor(16, b"BEA01")
        descriptor(17, b"NSR02")
        descriptor(18, b"TEA01")
        boot_record = 19
        tag = bytearray(16)
        struct.pack_into("<HH", tag, 0, 2, 2)
        struct.pack_into("<I", tag, 12, 256)
        tag[4] = sum(tag) & 0xFF
        image[256 * BLOCK:256 * BLOCK + len(tag)] = tag
    else:
        descriptor(16, b"CD001", 1)
        struct.pack_into("<I", image, 16 * BLOCK + 80, blocks)
        struct.pack_into(">I", image, 16 * BLOCK + 84, blocks)
        struct.pack_into("<H", image, 16 * BLOCK + 128, BLOCK)
        struct.pack_into(">H", image, 16 * BLOCK + 130, BLOCK)
        boot_record = 17
        descriptor(18, b"CD001", 255)

    descriptor(boot_record, b"CD001")
    start = boot_record * BLOCK
    image[start + 7:start + 30] = b"EL TORITO SPECIFICATION"
    struct.pack_into("<I", image, start + 71, 24)

    entry = 24 * BLOCK
    image[entry] = 1
    image[entry + 1] = 0 if bios else 0xEF
    image[entry + 30:entry + 32] = b"\x55\xaa"
    checksum = sum(struct.unpack_from("<16H", image, entry))
    struct.pack_into("<H", image, entry + 28, (-checksum) & 0xFFFF)

    image[entry + 32] = 0x88
    struct.pack_into("<HI", image, entry + 38, 4 if bios else 0, 27)
    if bios and efi:
        image[entry + 64:entry + 66] = b"\x91\xef"
        struct.pack_into("<H", image, entry + 66, 1)
        image[entry + 96] = 0x88
        struct.pack_into("<HI", image, entry + 102, 8, 29)
    return image


class OpticalImageTest(unittest.TestCase):
    def test_iso_bios_and_uefi(self):
        data = image_bytes(efi=True)
        result = inspect_image(BytesIO(data))
        self.assertEqual(result.formats, ("ISO 9660",))
        self.assertEqual(result.platforms, ("BIOS", "UEFI"))
        self.assertEqual(result.crc32, zlib.crc32(data))

    def test_udf_uefi_only(self):
        result = inspect_image(BytesIO(image_bytes(udf=True, bios=False)))
        self.assertEqual(result.formats, ("UDF",))
        self.assertEqual(result.platforms, ("UEFI",))

    def test_wire_encapsulation_round_trip(self):
        data = image_bytes(udf=True, efi=True)
        stream = BytesIO(data)
        result = inspect_image(stream)
        name = "부팅.iso".encode()
        begin = result.begin_payload(name.decode())
        self.assertEqual(struct.unpack_from("<QI28sBBH", begin),
                         (len(data), zlib.crc32(data), bytes(28), 1, 0, len(name)))
        self.assertEqual(begin[44:], name)
        reassembled = bytearray()
        for payload in upload_chunks(stream, result, 17):
            self.assertLessEqual(len(payload), 12 + CHUNK)
            self.assertEqual(struct.unpack_from("<IQ", payload), (17, len(reassembled)))
            reassembled.extend(payload[12:])
        self.assertEqual(reassembled, data)

    def test_rejects_raw_and_truncated(self):
        for data in (bytes(40 * BLOCK), image_bytes()[:-1]):
            with self.assertRaises(ImageError):
                inspect_image(BytesIO(data))

    def test_rejects_missing_boot_catalog_checksum_or_anchor(self):
        no_boot = image_bytes()
        struct.pack_into("<I", no_boot, 17 * BLOCK + 71, 0)
        checksum = image_bytes()
        checksum[24 * BLOCK + 28] ^= 1
        anchor = image_bytes(udf=True)
        anchor[256 * BLOCK + 4] ^= 1
        for data in (no_boot, checksum, anchor):
            with self.assertRaises(ImageError):
                inspect_image(BytesIO(data))

    def test_rejects_boot_image_outside_media(self):
        data = image_bytes()
        struct.pack_into("<I", data, 24 * BLOCK + 40, 40)
        with self.assertRaises(ImageError):
            inspect_image(BytesIO(data))

    def test_name_and_size_validation(self):
        result = inspect_image(BytesIO(image_bytes()))
        for name in ("", "dir/image.iso", "x\u0000.iso", "가" * 41):
            with self.assertRaises(ImageError):
                result.begin_payload(name)
        with self.assertRaises(ImageError):
            list(upload_chunks(BytesIO(image_bytes()[:-1]), result, 1))
        with self.assertRaises(ImageError):
            list(upload_chunks(BytesIO(image_bytes() + b"x"), result, 1))


if __name__ == "__main__":
    unittest.main()

