"""Build a verified 512-byte-sector filesystem image on the management host."""

import argparse
from pathlib import Path
import os
import secrets
import shutil
import struct
import subprocess
import tempfile

SECTOR = 512
PARTITION_LBA = 2048
PARTITION_TYPE = {"fat32": 0x0C, "exfat": 0x07, "ext4": 0x83}
BUFFER = 64 * 1024
FORMATS = {
    "ext4": ("mke2fs", "-q", "-F", "-t", "ext4", "-m", "0",
             "-E", "lazy_itable_init=0,lazy_journal_init=0,nodiscard"),
    "exfat": ("mkfs.exfat",),
    "fat32": ("mkfs.fat", "-F", "32", "-s", "1"),
}
CHECKS = {
    "ext4": ("e2fsck", "-fn"),
    "exfat": ("fsck.exfat", "-n"),
    "fat32": ("fsck.fat", "-n"),
}


def read_sectors(stream, length):
    data = stream.read(length)
    if len(data) != length:
        raise OSError("disk image size changed while reading")
    return data


def changed_sectors(before, after, size, on_scan=None):
    """Yield (LBA, data) for changed sectors. A missing prior image means all zero."""
    if size <= 0 or size % SECTOR:
        raise ValueError("size must be a positive multiple of 512 bytes")
    after.seek(0)
    if before is not None:
        before.seek(0)
    empty = bytes(BUFFER)
    for base in range(0, size, BUFFER):
        if on_scan is not None:
            on_scan()
        length = min(BUFFER, size - base)
        modified = read_sectors(after, length)
        original = read_sectors(before, length) if before is not None else empty[:length]
        if modified == original:
            continue
        for offset in range(0, length, SECTOR):
            sector = modified[offset:offset + SECTOR]
            if sector != original[offset:offset + SECTOR]:
                yield (base + offset) // SECTOR, sector
    if after.read(1) or (before is not None and before.read(1)):
        raise OSError("disk image grew while reading")


def run(command, *paths):
    result = subprocess.run([*command, *(str(path) for path in paths)], check=False, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, errors="replace")
    if result.returncode:
        raise RuntimeError(f"{command[0]} failed ({result.returncode})\n{result.stdout.strip()}")
    return result.stdout


def check_filesystem(image, filesystem):
    detected = run(("blkid", "-p", "-o", "value", "-s", "TYPE"), image).strip()
    expected = "vfat" if filesystem == "fat32" else filesystem
    if detected != expected:
        raise ValueError(f"formatter returned {detected!r}, expected {expected!r}")
    with image.open("rb") as stream:
        boot = read_sectors(stream, SECTOR)
    if filesystem == "fat32":
        sector_size = struct.unpack_from("<H", boot, 11)[0]
        spc = boot[13]
        reserved, fat_count, root_entries, total16 = struct.unpack_from("<HBHH", boot, 14)
        small_fat = struct.unpack_from("<H", boot, 22)[0]
        total, fat_size = struct.unpack_from("<II", boot, 32)
        data_sectors = total - reserved - fat_count * fat_size
        if (sector_size != SECTOR or spc == 0 or reserved == 0 or fat_count == 0
                or root_entries or total16 or small_fat or not fat_size
                or data_sectors // spc < 65525):
            raise ValueError("image is too small for this FAT32 geometry")
    if filesystem == "exfat" and boot[108] != 9:
        raise ValueError("formatter used a sector size other than 512 bytes")
    run(CHECKS[filesystem], image)


def update_exfat_boot_regions(original, output):
    # exFAT 1.00 main and backup boot regions each occupy 12 logical sectors.
    for _ in range(2):
        region = bytearray(read_sectors(original, 12 * SECTOR))
        struct.pack_into("<Q", region, 64, PARTITION_LBA)
        checksum = 0
        for offset, byte in enumerate(region[:11 * SECTOR]):
            if offset not in (106, 107, 112):  # VolumeFlags and PercentInUse
                checksum = (((checksum >> 1) | ((checksum & 1) << 31)) + byte) & 0xFFFFFFFF
        region[11 * SECTOR:] = struct.pack("<I", checksum) * (SECTOR // 4)
        output.write(region)


def pack_image(volume, output, filesystem, volume_bytes):
    """Add one 1 MiB-aligned MBR partition around a checked filesystem."""
    sectors = volume_bytes // SECTOR
    if sectors + PARTITION_LBA > 0xFFFFFFFF:
        raise ValueError("MBR disk exceeds the 32-bit sector limit")
    mbr = bytearray(SECTOR)
    struct.pack_into("<I", mbr, 440, 1 + secrets.randbelow(0xFFFFFFFF))
    struct.pack_into("<B3sB3sII", mbr, 446, 0, bytes.fromhex("202100"), PARTITION_TYPE[filesystem],
                     bytes.fromhex("feffff"), PARTITION_LBA, sectors)
    mbr[510:] = bytes.fromhex("55aa")
    with volume.open("rb") as original, output.open("xb") as disk:
        disk.write(mbr)
        disk.seek(PARTITION_LBA * SECTOR)
        if filesystem == "exfat":
            update_exfat_boot_regions(original, disk)
        shutil.copyfileobj(original, disk, 1024 * 1024)
    return volume_bytes + PARTITION_LBA * SECTOR


def build_image(output, filesystem, mib, files=(), *, partitioned=False):
    sources = {}
    for item in files:
        name = item.name
        member = item.resolve(strict=True)
        key = name if filesystem == "ext4" else name.casefold()
        if key in sources or not member.is_file() or name in ("", ".", ".."):
            raise ValueError(f"invalid or duplicate input file: {item}")
        sources[key] = name, member
    if mib <= 0:
        raise ValueError("disk image size must be positive")
    size = mib * 1024 * 1024
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise FileExistsError(output)
    with tempfile.TemporaryDirectory(prefix=".minikmb-disk-", dir=output.parent) as temp:
        staging = Path(temp) / "image.img"
        with staging.open("xb") as disk:
            disk.truncate(size)
        command = FORMATS[filesystem]
        if filesystem == "ext4" and sources:
            tree = Path(temp) / "files"
            tree.mkdir()
            for name, source in sources.values():
                shutil.copyfile(source, tree / name)
            command = (*command, "-d", str(tree))
        if filesystem == "fat32" and partitioned:
            command = (*command, "-h", str(PARTITION_LBA))
        run(command, staging)
        if filesystem != "ext4" and sources:
            # Build the checked-in file-image adapter on the management host.
            script = Path(__file__).resolve().with_name("build_fat_image_io.sh")
            with tempfile.TemporaryDirectory(prefix="minikmb-fat-tool-") as tool_dir:
                fat = Path(tool_dir) / "fat-image-io"
                run(("bash", str(script)), fat)
                for name, source in sources.values():
                    run((str(fat), "put"), staging, name, source)
        check_filesystem(staging, filesystem)
        if staging.stat().st_size != size:
            raise ValueError("formatter changed the disk capacity")
        if partitioned:
            packed = Path(temp) / "partitioned.img"
            size = pack_image(staging, packed, filesystem, size)
            staging = packed
        os.link(staging, output)
    return size


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("output", type=Path)
    cli.add_argument("--format", choices=tuple(FORMATS), required=True)
    cli.add_argument("--size-mib", type=int, default=64)
    cli.add_argument("--include", type=Path, action="append", default=[], help="copy a file to the image root")
    cli.add_argument("--partitioned", action="store_true", help="add an MBR for OS-visible USB disks")
    args = cli.parse_args()
    try:
        size = build_image(args.output, args.format, args.size_mib, args.include, partitioned=args.partitioned)
    except (OSError, ValueError, RuntimeError) as error:
        cli.exit(2, f"{error}\n")
    print(f"{args.format} image ready: {size} bytes, {size // SECTOR} sectors")


if __name__ == "__main__":
    main()
