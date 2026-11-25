"""Bounded, offline helpers for the Windows preview archive."""
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import tarfile
import tomllib
import zipfile

MAX_FILE = 160 * 1024 * 1024
MAX_ARCHIVE = 512 * 1024 * 1024


def sha(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(value):
    return (json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + "\n").encode("utf-8")


def safe_name(name):
    if not name or "\\" in name or ":" in name or name.startswith("/"):
        raise ValueError(f"Unsafe archive path: {name}")
    parts = name.split("/")
    if any(p in ("", ".", "..") for p in parts):
        raise ValueError(f"Unsafe archive path: {name}")
    reserved = {"CON", "PRN", "AUX", "NUL"} | {f"{prefix}{n}" for prefix in ["COM", "LPT"] for n in range(1, 10)}
    if any(p.endswith((".", " ")) or p.split(".")[0].upper() in reserved for p in parts):
        raise ValueError(f"Unsafe Windows archive path: {name}")
    return name


def dependency_closure(metadata):
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    seen, todo = set(), [metadata["resolve"]["root"]]
    while todo:
        key = todo.pop()
        if key in seen:
            continue
        seen.add(key)
        todo.extend(d["pkg"] for d in nodes[key]["deps"]
                    if any(k["kind"] != "dev" for k in d["dep_kinds"]))
    return seen


def upstream_for(package):
    name, version = package["name"], package["version"]
    if name == "accesskit" and version == "0.21.1":
        return "accesskit-0.21.1"
    if (name, version) in [("accesskit_consumer", "0.31.0"), ("accesskit_windows", "0.29.2"), ("accesskit_winit", "0.29.2")]:
        return "accesskit-0.29.2"
    if version == "0.33.3" and name in {"ecolor", "eframe", "egui", "egui-wgpu", "egui-winit", "emath", "epaint", "epaint_default_fonts"}:
        return "egui-0.33.3"
    if (name, version) in [("clipboard-win", "5.4.3"), ("hexf-parse", "0.2.1"), ("profiling", "1.0.18")]:
        return f"{name}-{version}"
    return None


def crate_members(blob, prefix, notices_only=False):
    result = {}
    with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as archive:
        total = 0
        for member in archive:
            # No extraction. Symlinks/special files are never used as notices.
            if not member.isfile():
                continue
            name = safe_name(member.name)
            if not name.startswith(prefix + "/"):
                raise ValueError("Unexpected crate archive prefix")
            relative = name[len(prefix) + 1:]
            if member.size > MAX_FILE or total + member.size > MAX_ARCHIVE:
                raise ValueError("Crate source exceeds bound")
            total += member.size
            if notices_only and relative != ".cargo_vcs_info.json" and not notice_file(relative):
                continue
            if relative in result:
                raise ValueError("Duplicate crate member")
            stream = archive.extractfile(member)
            if stream is None:
                raise ValueError("Missing crate member")
            result[relative] = stream.read(member.size + 1)
            if len(result[relative]) != member.size:
                raise ValueError("Truncated crate member")
    return result


def notice_file(name):
    path = PurePosixPath(name)
    lower = path.name.lower()
    return (lower.startswith(("license", "licence", "copying", "notice", "copyright", "ofl", "ufl"))
            or "license" in lower or lower.startswith("readme")
            or ("fonts" in path.parts and path.suffix.lower() == ".txt"))


def collect_notices(root, metadata, rust_sysroot):
    """Conservative Windows normal+build graph; deliberately excludes dev-only packages."""
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    checksums = {(p["name"], p["version"], p.get("source")): p.get("checksum") for p in lock["package"]}
    upstream = json.loads((root / "licenses/upstream.json").read_text(encoding="utf-8"))["files"]
    packages = {p["id"]: p for p in metadata["packages"]}
    selected = dependency_closure(metadata)
    files, rows = {}, []
    for key in sorted(selected, key=lambda k: (packages[k]["name"], packages[k]["version"])):
        p = packages[key]
        if key == metadata["resolve"]["root"]:
            continue
        if not p["source"] or not p["source"].startswith("registry+"):
            raise ValueError(f"Unreviewed dependency source: {p['name']}")
        if not p["license"]:
            raise ValueError(f"Missing declared license: {p['name']}")
        source = Path(p["manifest_path"]).parent
        name = f"{p['name']}-{p['version']}"
        archive_path = source.parents[2] / "cache" / source.parent.name / f"{name}.crate"
        blob = archive_path.read_bytes()
        checksum = checksums.get((p["name"], p["version"], p["source"]))
        if not checksum or sha(blob) != checksum:
            raise ValueError(f"Cargo.lock source checksum mismatch: {name}")
        members = crate_members(blob, name, notices_only="MPL-2.0" not in p["license"])
        notices = []
        for relative, data in sorted(members.items()):
            if notice_file(relative):
                if len(data) > 2 * 1024 * 1024:
                    raise ValueError("Notice file exceeds bound")
                destination = safe_name(f"licenses/crates/{name}/{relative}")
                files[destination] = data
                basename = PurePosixPath(relative).name.lower()
                license_text = ("license" in basename or basename.startswith(("licence", "copying", "ofl", "ufl")))
                notices.append({"path": destination, "sha256": sha(data), "origin": "locked crate archive", "license_text": license_text})
        group = upstream_for(p)
        for u in upstream:
            if group and u["file"].startswith(group + "/"):
                data = (root / "licenses/upstream" / u["file"]).read_bytes()
                if sha(data) != u["sha256"]:
                    raise ValueError("Pinned upstream license hash mismatch")
                vcs = json.loads(members.get(".cargo_vcs_info.json", b"{}"))
                if u["commit"] and vcs.get("git", {}).get("sha1") != u["commit"]:
                    raise ValueError(f"Upstream license commit mismatch: {name}")
                destination = safe_name(f"licenses/crates/{name}/upstream-{PurePosixPath(u['file']).name}")
                files[destination] = data
                notices.append({"path": destination, "sha256": sha(data), "origin": u["url"], "license_text": True})
        # A README by itself is not a replacement for the license text.
        if not any(n["license_text"] for n in notices):
            raise ValueError(f"No license text collected: {name}")
        row = {"name": p["name"], "version": p["version"], "license": p["license"],
               "authors": p.get("authors", []), "repository": p["repository"], "source_checksum": checksum,
               "source_url": f"https://static.crates.io/crates/{p['name']}/{name}.crate", "notices": notices}
        if "MPL-2.0" in p["license"]:
            # Supply the exact unchanged source and check the source used locally.
            for relative, data in members.items():
                cached = source / relative
                if not cached.is_file() or cached.read_bytes() != data:
                    raise ValueError(f"MPL source was locally changed: {name}/{relative}")
            destination = f"sources/{name}.crate"
            files[destination] = blob
            row["bundled_source"] = destination
        rows.append(row)
    for path in sorted((rust_sysroot / "share/doc/rust/licenses").glob("*.txt")):
        files["licenses/rust/" + path.name] = path.read_bytes()
    copyright_path = rust_sysroot / "share/doc/rust/COPYRIGHT-library.html"
    files["licenses/rust/COPYRIGHT-library.html"] = copyright_path.read_bytes()
    if "licenses/rust/MIT.txt" not in files or "licenses/rust/Apache-2.0.txt" not in files:
        raise ValueError("Rust standard library notices are missing")
    for path in sorted((root / "assets/fonts").glob("*.txt")):
        files["licenses/ui/" + path.name] = path.read_bytes()
    files["licenses/ui/manifest.json"] = (root / "assets/manifest.json").read_bytes()
    inventory = {"platform": "x86_64-pc-windows-msvc", "scope": "Windows normal and build dependency closure, excluding dev-only; not a claim that every package is linked",
                 "cargo_lock_sha256": sha((root / "Cargo.lock").read_bytes()), "packages": rows}
    return files, inventory


def pe_imports(data):
    """Inspect direct/delay PE imports without loading or running the binary."""
    def read(fmt, at):
        size = struct.calcsize(fmt)
        if at < 0 or at + size > len(data):
            raise ValueError("Truncated PE data")
        return struct.unpack_from(fmt, data, at)
    if data[:2] != b"MZ":
        raise ValueError("Not a Windows executable")
    pe = read("<I", 0x3c)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("Missing PE signature")
    machine, count, _, _, _, optional_size, _ = read("<HHIIIHH", pe + 4)
    optional = pe + 24
    if machine != 0x8664 or count > 96 or optional_size < 112 or read("<H", optional)[0] != 0x20b:
        raise ValueError("Expected a bounded x64 PE32+ executable")
    image_base = read("<Q", optional + 24)[0]
    headers = read("<I", optional + 60)[0]
    subsystem = read("<H", optional + 68)[0]
    directories = min(read("<I", optional + 108)[0], 16)
    if optional_size < 112 + directories * 8:
        raise ValueError("Invalid PE data directories")
    sections = []
    for index in range(count):
        at = optional + optional_size + 40 * index
        virtual_size, va, raw_size, raw = read("<IIII", at + 8)
        if raw + raw_size > len(data):
            raise ValueError("PE section points outside file")
        sections.append((va, max(virtual_size, raw_size), raw_size, raw))
    def offset(rva):
        if 0 <= rva < min(headers, len(data)):
            return rva
        for va, size, raw_size, raw in sections:
            if va <= rva < va + size:
                if rva - va >= raw_size:
                    raise ValueError("PE reference points into uninitialized data")
                return raw + rva - va
        raise ValueError("Unmapped PE address")
    def dll_name(rva):
        at = offset(rva)
        end = data.find(b"\0", at, min(at + 260, len(data)))
        if end < 0:
            raise ValueError("Unterminated PE DLL name")
        name = data[at:end].decode("ascii").lower()
        if not re.fullmatch(r"[a-z0-9_.-]+\.(dll|drv)", name):
            raise ValueError("Unsafe PE DLL name")
        return name
    found = {"direct": set(), "delay": set()}
    for entry, kind, width in [(1, "direct", 20), (13, "delay", 32)]:
        if entry >= directories:
            continue
        rva, size = read("<II", optional + 112 + entry * 8)
        if not rva and not size:
            continue
        if size < width or size // width > 4096:
            raise ValueError("Invalid PE import directory size")
        terminated = False
        for index in range(size // width):
            fields = read("<" + "I" * (width // 4), offset(rva + width * index))
            if not any(fields):
                terminated = True
                break
            name_rva = fields[3] if kind == "direct" else fields[1]
            if kind == "delay" and not fields[0] & 1:
                name_rva -= image_base
            found[kind].add(dll_name(name_rva))
        if not terminated:
            raise ValueError("Unterminated PE imports")
    return {"machine": "x86_64", "subsystem": subsystem, **{k: sorted(v) for k, v in found.items()},
            "scope": "PE direct/delay imports only; dynamically loaded OS components also need runtime validation"}


def input_digest(root):
    paths = [root / name for name in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml"]]
    for directory in ["src", "assets", "firmware-tools", "native-keyboard"]:
        paths.extend(p for p in (root / directory).rglob("*") if p.is_file() and "__pycache__" not in p.parts and p.suffix != ".pyc")
    digest = hashlib.sha256()
    for path in sorted(paths):
        relative = path.relative_to(root).as_posix()
        digest.update(relative.encode("utf-8") + b"\0" + hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def write_zip(destination, payload, info):
    payload = dict(payload)
    if "manifest.json" in payload:
        raise ValueError("Manifest is reserved")
    manifest = {**info, "files": {safe_name(k): {"sha256": sha(v), "bytes": len(v)} for k, v in sorted(payload.items())}}
    payload["manifest.json"] = json_bytes(manifest)
    folded = set()
    for name in payload:
        safe_name(name)
        if name != "manifest.json" and not allowed_payload(name):
            raise ValueError("Unexpected release payload")
        if name.casefold() in folded:
            raise ValueError("Case-insensitive ZIP path collision")
        folded.add(name.casefold())
    # Deterministic member order/timestamps; no local cache/configuration paths.
    with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data in sorted(payload.items()):
            entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = (stat.S_IFREG | 0o644) << 16
            archive.writestr(entry, data)


def verify_zip(path):
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        if len(entries) > 8192 or sum(e.file_size for e in entries) > MAX_ARCHIVE:
            raise ValueError("ZIP exceeds total bound")
        folded = set()
        for entry in entries:
            safe_name(entry.filename)
            if entry.filename != "manifest.json" and not allowed_payload(entry.filename):
                raise ValueError("Unexpected release payload")
            if (entry.file_size > MAX_FILE or entry.flag_bits & 1 or stat.S_ISLNK(entry.external_attr >> 16)
                    or entry.filename.casefold() in folded):
                raise ValueError("Invalid, duplicate, encrypted or oversized ZIP entry")
            folded.add(entry.filename.casefold())
        if archive.getinfo("manifest.json").file_size > 1024 * 1024:
            raise ValueError("Manifest exceeds bound")
        manifest = json.loads(archive.read("manifest.json"))
        if set(archive.namelist()) != set(manifest["files"]) | {"manifest.json"}:
            raise ValueError("ZIP contains missing or unlisted files")
        for name, record in manifest["files"].items():
            safe_name(name)
            digest = hashlib.sha256()
            total = 0
            with archive.open(name) as source:
                for chunk in iter(lambda: source.read(1024 * 1024), b""):
                    total += len(chunk)
                    digest.update(chunk)
            if total != record["bytes"] or digest.hexdigest() != record["sha256"]:
                raise ValueError("ZIP file hash/length mismatch")
        return manifest


def allowed_payload(name):
    return (name in {"minikmb-native.exe", "minikmb-cli.exe", "CLI.md", "CLI.en.md", "cli-runtime-imports.json", "README.txt", "LICENSE.txt", "THIRD-PARTY.md", "dependencies.json", "runtime-imports.json", "Cargo.lock", "rustc-version.txt", "assets-manifest.json"}
            or name in {"tools/probe-rs.exe", "tools/minikmb-ms2130-helper.exe", "tools/manifest.json",
                        "tools/minikmb-keyboard.dll", "tools/keyboard-helper.json", "sources/minikmb-keyboard.cpp", "sources/build-native-keyboard.py",
                        "firmware/MiniKMB-FS-HID-r63.elf", "firmware/MS2130_sample_20221026.bin", "firmware/manifest.json",
                        "sources/minikmb-ms2130-helper-source.tar.gz"}
            or name.startswith("licenses/") or bool(re.fullmatch(r"sources/[a-z0-9_-]+-[0-9][a-zA-Z0-9.+_-]*\.crate", name)))
