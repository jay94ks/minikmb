"""Check TinyUSB symbol separation in a CubeIDE ARM ELF build."""

from pathlib import Path
import argparse
import re
import struct


def elf_symbols(path):
    raw = path.read_bytes()
    if raw[:6] != b"\x7fELF\x01\x01":
        raise ValueError(f"expected ELF32 little-endian: {path}")
    offset, = struct.unpack_from("<I", raw, 32)
    size, count = struct.unpack_from("<HH", raw, 46)
    sections = [struct.unpack_from("<10I", raw, offset + n * size) for n in range(count)]
    for header in sections:
        if header[1] != 2:
            continue
        strings = sections[header[6]]
        names = raw[strings[4]:strings[4] + strings[5]]
        for position in range(header[4], header[4] + header[5], header[9]):
            start, address, length, info, _, index = struct.unpack_from("<IIIBBH", raw, position)
            if info >> 4 in (1, 2) and start:
                name = names[start:].split(b"\0", 1)[0].decode("ascii")
                yield name, index != 0, address, length


def object_symbols(files):
    definitions, references = set(), set()
    for path in files:
        for name, defined, _, _ in elf_symbols(path):
            (definitions if defined else references).add(name)
    return definitions, references


def audit(project, directory):
    build = project / directory
    objects = [Path(line.strip().strip('"')) for line in (build / "objects.list").read_text().splitlines()
               if line.strip()]
    fs_paths = [build / path for path in objects
                if path.parts[:3] == ("Middlewares", "Third_Party", "TinyUSB")]
    hs_paths = [build / path for path in objects
                if path.parent == Path("Core/Src/usb/target_stack")]
    required = {"cdc_device.o", "dcd_dwc2.o", "dwc2_common.o", "tusb.o", "tusb_fifo.o", "usbd.o"}
    fs_names = {path.name for path in fs_paths}
    hs_names = {path.name.removeprefix("target_") for path in hs_paths}
    allowed = required | {"hid_device.o", "msc_device.o", "ncm_device.o"}
    if (not required <= fs_names <= allowed or not required <= hs_names <= allowed or
            len(fs_names) != len(fs_paths) or len(hs_names) != len(hs_paths) or
            any(not path.name.startswith("target_") for path in hs_paths)):
        raise ValueError("unexpected TinyUSB link inputs")
    for path in fs_paths:
        if path.name in {"hid_device.o", "ncm_device.o"} and any(object_symbols([path])):
            raise ValueError(f"{path.name} must be compiled out of the management stack")
    fs, fs_ref = object_symbols(fs_paths)
    hs, hs_ref = object_symbols(hs_paths)
    if fs & hs:
        raise ValueError(f"overlapping TinyUSB definitions: {sorted(fs & hs)}")
    if fs_ref & hs or hs_ref & fs:
        raise ValueError(f"cross-instance references: {sorted((fs_ref & hs) | (hs_ref & fs))}")
    for name in hs:
        if not name.startswith("target_") and name != "TargetMsc_ReadBuffer":
            raise ValueError(f"target stack defines raw global {name}")
    callbacks = {"tud_descriptor_device_cb", "tud_descriptor_configuration_cb",
                 "tud_descriptor_string_cb"}
    if callbacks & hs_ref:
        raise ValueError("target stack references FS descriptor callbacks")

    image = {name: address for name, defined, address, _ in elf_symbols(build / "KM_H743.elf")
             if defined}
    for name in ("tud_rhport_init", "dcd_int_handler", "tud_descriptor_device_cb",
                 "tud_descriptor_configuration_cb", "tud_descriptor_string_cb"):
        if image[name] == image["target_" + name]:
            raise ValueError(f"shared implementation: {name}")
    if "target_hidd_open" in hs:
        for name in ("hidd_open", "tud_hid_n_report", "tud_hid_descriptor_report_cb",
                     "tud_hid_report_complete_cb", "tud_hid_report_failed_cb"):
            if name in image or "target_" + name not in image:
                raise ValueError(f"HID symbol was not isolated to the target: {name}")
    if "target_netd_open" in hs:
        for name in ("netd_open", "tud_network_link_state", "tud_network_recv_cb"):
            if name in image or "target_" + name not in image:
                raise ValueError(f"NCM symbol was not isolated to the target: {name}")
    if "mscd_open" in fs and "target_mscd_open" in hs:
        if image["mscd_open"] == image["target_mscd_open"]:
            raise ValueError("shared MSC implementation across USB ports")
    for irq in ("OTG_FS_IRQHandler", "OTG_HS_IRQHandler"):
        if irq not in image or image[irq] == image.get("Default_Handler"):
            raise ValueError(f"missing distinct handler: {irq}")
    config = (build / "KM_H743.map").read_text()
    for port, start in [("fs", 0x24060000), ("hs", 0x24064000)]:
        match = re.search(rf"^\.usb_{port}_dma\s+(0x[0-9a-f]+)\s+(0x[0-9a-f]+)", config, re.M)
        if not match or int(match[1], 16) != start or int(match[2], 16) <= 0:
            raise ValueError(f"wrong endpoint buffer placement: {port}")
    print(f"FS {len(fs)} / HS {len(hs)} definitions, isolated callbacks, IRQs and endpoint buffers")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--build", default="Debug")
    args = parser.parse_args()
    audit(args.project, args.build)
