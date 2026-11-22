"""Round-trip a locally generated DEB and a supplied RPM through all image formats."""

import argparse
import hashlib
from pathlib import Path
import random
import shutil
import struct
import subprocess
import tempfile

from prepare_writable_disk import FORMATS, PARTITION_LBA, PARTITION_TYPE, SECTOR, build_image, check_filesystem


def build_deb(directory):
    tree = directory / "deb-tree"
    control = tree / "DEBIAN"
    control.mkdir(parents=True)
    (control / "control").write_text(
        "Package: minikmb-media-test\nVersion: 1.0\nArchitecture: all\n"
        "Maintainer: MiniKMB Test <test@example.invalid>\n"
        "Description: File transfer fixture\n",
        encoding="ascii",
    )
    data = tree / "usr/share/minikmb-media-test"
    data.mkdir(parents=True)
    (data / "sample.bin").write_bytes(random.Random(743).randbytes(180000))
    deb = directory / "minikmb-media-test_1.0_all.deb"
    subprocess.run(["dpkg-deb", "--build", tree, deb], check=True, capture_output=True)
    return deb


def test(rpm):
    if rpm.suffix.lower() != ".rpm" or rpm.read_bytes()[:4] != bytes.fromhex("ed ab ee db"):
        raise ValueError("provide a real RPM as the input fixture")
    with tempfile.TemporaryDirectory(prefix="minikmb-package-test-") as temp:
        directory = Path(temp)
        deb = build_deb(directory)
        files = [deb, rpm]
        expected = {file.name: hashlib.sha256(file.read_bytes()).digest() for file in files}
        bridge = directory / "fat-image-io"
        script = Path(__file__).resolve().with_name("build_fat_image_io.sh")
        subprocess.run(["bash", script, bridge], check=True)
        for filesystem in FORMATS:
            image = directory / f"{filesystem}.img"
            disk = directory / f"{filesystem}-mbr.img"
            size = build_image(disk, filesystem, 64, files, partitioned=True)
            with disk.open("rb") as stream, image.open("xb") as volume:
                mbr = stream.read(SECTOR)
                assert mbr[510:] == bytes.fromhex("55aa")
                assert mbr[450] == PARTITION_TYPE[filesystem]
                start, sectors = struct.unpack_from("<II", mbr, 454)
                assert start == PARTITION_LBA and size == (start + sectors) * SECTOR
                stream.seek(start * SECTOR)
                shutil.copyfileobj(stream, volume)
            check_filesystem(image, filesystem)
            copies = directory / filesystem
            copies.mkdir()
            for file in files:
                copy = copies / file.name
                if filesystem == "ext4":
                    subprocess.run(["debugfs", "-R", f"dump /{file.name} {copy}", image],
                                   check=True, capture_output=True)
                else:
                    subprocess.run([bridge, "get", image, file.name, copy], check=True)
                assert hashlib.sha256(copy.read_bytes()).digest() == expected[file.name]
            print(f"{filesystem} MBR: .deb and .rpm match")


if __name__ == "__main__":
    arguments = argparse.ArgumentParser(description=__doc__)
    arguments.add_argument("rpm", type=Path)
    test(arguments.parse_args().rpm)
