"""Prepare a common bootable optical image for MiniKMB management upload."""

import argparse
from dataclasses import dataclass
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
from typing import BinaryIO
import zlib

BLOCK = 2048
CHUNK = 1008
MEDIA_OPTICAL = 1
BIOS, UEFI = 0, 0xEF


class ImageError(ValueError):
    """The input is outside the MiniKMB optical image profile."""


@dataclass(frozen=True)
class OpticalImage:
    size: int
    formats: tuple[str, ...]
    platforms: tuple[str, ...]
    crc32: int

    def begin_payload(self, name: str) -> bytes:
        encoded = name.encode("utf-8")
        if not 1 <= len(encoded) <= 120 or any(
            ord(char) < 32 or 127 <= ord(char) <= 159 or char in "/\\" for char in name
        ):
            raise ImageError("invalid UTF-8 image display name")
        return struct.pack("<QI28xBBH", self.size, self.crc32, MEDIA_OPTICAL, 0, len(encoded)) + encoded


def sector(stream: BinaryIO, lba: int) -> bytes:
    stream.seek(lba * BLOCK)
    data = stream.read(BLOCK)
    if len(data) != BLOCK:
        raise ImageError("truncated optical block")
    return data


def anchor_present(stream: BinaryIO, blocks: int) -> bool:
    for lba in sorted({256, blocks - 257, blocks - 1}):
        if not 256 <= lba < blocks:
            continue
        tag = sector(stream, lba)[:16]
        if (tag[:2] == b"\x02\x00" and struct.unpack_from("<I", tag, 12)[0] == lba
                and (sum(tag) - tag[4]) & 0xFF == tag[4]):
            return True
    return False


def boot_platforms(catalog: bytes, blocks: int) -> tuple[str, ...]:
    validation = catalog[:32]
    if (validation[0] != 1 or validation[30:32] != b"\x55\xaa"
            or sum(struct.unpack("<16H", validation)) & 0xFFFF):
        raise ImageError("invalid El Torito catalog validation entry")
    selected = set()

    def check(entry: bytes, platform: int) -> None:
        if entry[0] != 0x88 or platform not in (BIOS, UEFI) or entry[1] != 0:
            return
        count, lba = struct.unpack_from("<HI", entry, 6)
        if not 0 < lba < blocks or lba * BLOCK + count * 512 > blocks * BLOCK:
            raise ImageError("El Torito boot image lies outside the optical image")
        if platform == BIOS and count == 0:
            raise ImageError("BIOS boot entry has an empty load size")
        selected.add(platform)

    check(catalog[32:64], validation[1])
    offset = 64
    while offset + 32 <= len(catalog):
        header = catalog[offset:offset + 32]
        if header == bytes(32):
            break
        if header[0] not in (0x90, 0x91):
            raise ImageError("unsupported El Torito catalog layout")
        count = struct.unpack_from("<H", header, 2)[0]
        offset += 32
        end = offset + count * 32
        if count == 0 or end > len(catalog):
            raise ImageError("El Torito section exceeds the one-block catalog profile")
        for index in range(offset, end, 32):
            check(catalog[index:index + 32], header[1])
        offset = end
        if header[0] == 0x91:
            break
    if not selected:
        raise ImageError("no BIOS or UEFI El Torito no-emulation boot entry")
    return tuple(name for code, name in ((BIOS, "BIOS"), (UEFI, "UEFI")) if code in selected)


def inspect_image(stream: BinaryIO) -> OpticalImage:
    """Check the host upload profile and hash the exact bytes that will be sent."""
    stream.seek(0, os.SEEK_END)
    size = stream.tell()
    if size % BLOCK or size <= 18 * BLOCK:
        raise ImageError("expected a 2,048-byte optical block image")
    blocks = size // BLOCK
    iso, iso_end = False, False
    vrs_state = 0
    catalog_lba = None
    for lba in range(16, min(blocks, 64)):
        data = sector(stream, lba)
        descriptor = data[1:6]
        if descriptor == b"CD001" and data[6] == 1:
            if data[0] == 1 and not iso_end:
                le_blocks, be_blocks = struct.unpack_from("<I", data, 80)[0], struct.unpack_from(">I", data, 84)[0]
                le_size, be_size = struct.unpack_from("<H", data, 128)[0], struct.unpack_from(">H", data, 130)[0]
                iso = 0 < le_blocks == be_blocks <= blocks and le_size == be_size == BLOCK
            elif data[0] == 255:
                iso_end = True
            if data[0] == 0 and data[7:39].rstrip(b"\0 ") == b"EL TORITO SPECIFICATION":
                candidate = struct.unpack_from("<I", data, 71)[0]
                if catalog_lba is not None and catalog_lba != candidate:
                    raise ImageError("conflicting El Torito boot catalogs")
                catalog_lba = candidate
        elif data[0] == 0 and data[6] == 1:
            if descriptor == b"BEA01":
                vrs_state = 1
            elif descriptor in (b"NSR02", b"NSR03") and vrs_state == 1:
                vrs_state = 2
            elif descriptor == b"TEA01" and vrs_state == 2:
                vrs_state = 3
    formats = []
    if iso and iso_end:
        formats.append("ISO 9660")
    if vrs_state == 3 and anchor_present(stream, blocks):
        formats.append("UDF")
    if not formats:
        raise ImageError("no ISO 9660 or UDF volume structure in the supported profile")
    if catalog_lba is None or not 16 < catalog_lba < blocks:
        raise ImageError("missing or out-of-range El Torito catalog")
    platforms = boot_platforms(sector(stream, catalog_lba), blocks)
    stream.seek(0)
    crc = 0
    length = 0
    while data := stream.read(1024 * 1024):
        crc = zlib.crc32(data, crc)
        length += len(data)
    if length != size:
        raise ImageError("image size changed during inspection")
    stream.seek(0)
    return OpticalImage(size, tuple(formats), platforms, crc)


def upload_chunks(stream: BinaryIO, image: OpticalImage, upload_id: int):
    """Yield v1 UPLOAD_DATA payloads. The target stores the original optical blocks."""
    if not 0 < upload_id <= 0xFFFFFFFF:
        raise ValueError("invalid upload ID")
    stream.seek(0)
    offset = 0
    while offset < image.size:
        expected = min(CHUNK, image.size - offset)
        data = stream.read(expected)
        if len(data) != expected:
            raise ImageError("image truncated during upload")
        yield struct.pack("<IQ", upload_id, offset) + data
        offset += expected
    if stream.read(1):
        raise ImageError("image grew during upload")


def build_image(args) -> OpticalImage:
    source = args.source.resolve(strict=True)
    output = args.output.absolute()
    if not source.is_dir():
        raise ImageError("source must be a prepared file tree")
    if output.exists() or output.is_symlink():
        raise ImageError("output already exists")
    if output.resolve().is_relative_to(source):
        raise ImageError("output must be outside the input tree")
    entries = []
    for platform, boot in (("0", args.bios_boot), ("EF", args.uefi_boot)):
        if boot is None:
            continue
        boot = boot.resolve(strict=True)
        if not boot.is_file() or any(char in str(boot) for char in "#,"):
            raise ImageError("use a boot-image file with no # or comma in its path")
        entries.append(f"p{platform},e,b{boot}")
    if not entries:
        raise ImageError("provide a BIOS or UEFI boot image")
    options = ["-u2", "-udfver102"] if args.format == "udf" else ["-n"]
    bootdata = f"-bootdata:{len(entries)}#" + "#".join(entries)
    with tempfile.TemporaryDirectory(prefix=".minikmb-", dir=output.parent) as temp:
        staging = Path(temp) / "image.iso"
        subprocess.run([args.oscdimg, "-m", "-o", *options, bootdata, str(source), str(staging)],
                       check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        with staging.open("rb") as stream:
            image = inspect_image(stream)
        expected = "UDF" if args.format == "udf" else "ISO 9660"
        if expected not in image.formats:
            raise ImageError(f"authoring tool output lacks {expected}")
        image.begin_payload(output.name)
        os.link(staging, output)
    return image


def main() -> None:
    cli = argparse.ArgumentParser(description=__doc__)
    modes = cli.add_subparsers(dest="command", required=True)
    check = modes.add_parser("check", help="inspect an existing prepared image")
    check.add_argument("image", type=Path)
    build = modes.add_parser("build", help="author a boot image with Windows ADK Oscdimg")
    build.add_argument("source", type=Path)
    build.add_argument("output", type=Path)
    build.add_argument("--format", choices=("iso9660", "udf"), required=True)
    build.add_argument("--bios-boot", type=Path)
    build.add_argument("--uefi-boot", type=Path)
    build.add_argument("--oscdimg", default="oscdimg.exe", help="path or command name")
    args = cli.parse_args()
    try:
        if args.command == "build":
            info = build_image(args)
            path = args.output
        else:
            path = args.image
            with path.open("rb") as stream:
                info = inspect_image(stream)
        info.begin_payload(path.name)
        print(json.dumps({"image": str(path), "bytes": info.size, "formats": info.formats,
                          "boot": info.platforms, "crc32": f"{info.crc32:08x}",
                          "media_kind": MEDIA_OPTICAL}, ensure_ascii=False))
    except subprocess.CalledProcessError as exc:
        cli.exit(2, f"Oscdimg exited {exc.returncode}\n{exc.stdout.decode(errors='replace')}\n")
    except (OSError, ImageError) as exc:
        cli.exit(2, f"{exc}\n")


if __name__ == "__main__":
    main()

