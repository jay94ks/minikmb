"""Packaging tests use only generated files, in-memory archives and PE fixtures."""
import io
import json
from pathlib import Path
import stat
import struct
import tarfile
import tempfile
import unittest
import zipfile
from release_support import cli_documents, collect_notices, crate_members, dependency_closure, json_bytes, pe_imports, safe_name, sha, verify_zip, write_zip


def archive_bytes(members):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for name, data in members.items():
            entry = tarfile.TarInfo(name)
            entry.size = len(data)
            archive.addfile(entry, io.BytesIO(data))
    return output.getvalue()


def pe_fixture(delay=False):
    data = bytearray(0x600)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3c, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<HHIIIHH", data, 0x84, 0x8664, 1, 0, 0, 0, 240, 0)
    optional = 0x98
    struct.pack_into("<H", data, optional, 0x20b)
    struct.pack_into("<Q", data, optional + 24, 0x140000000)
    struct.pack_into("<I", data, optional + 60, 0x200)
    struct.pack_into("<H", data, optional + 68, 2)
    struct.pack_into("<I", data, optional + 108, 16)
    struct.pack_into("<IIII", data, optional + 240 + 8, 0x400, 0x1000, 0x400, 0x200)
    if delay:
        struct.pack_into("<II", data, optional + 112 + 13 * 8, 0x1000, 64)
        struct.pack_into("<8I", data, 0x200, 1, 0x1080, 0, 0, 0, 0, 0, 0)
    else:
        struct.pack_into("<II", data, optional + 120, 0x1000, 40)
        struct.pack_into("<5I", data, 0x200, 0, 0, 0, 0x1080, 0)
    data[0x280:0x28d] = b"KERNEL32.dll\0"
    return bytes(data)


class PackagingTests(unittest.TestCase):
    def test_cli_agent_and_user_documents_are_required_and_hash_verified(self):
        root = Path(__file__).resolve().parents[1]
        documents = cli_documents(root)
        self.assertEqual(set(documents), {"SKILL.md", "CLI.md", "CLI.en.md", "REST-API.md", "openapi.json", "REST-API.html"})
        output = io.BytesIO()
        write_zip(output, documents, {"channel": "test-fixture"})
        manifest = verify_zip(io.BytesIO(output.getvalue()))
        self.assertEqual(manifest["files"]["SKILL.md"]["sha256"],
                         sha((root / "packaging/SKILL.md").read_bytes()))
        for name, data in documents.items():
            self.assertEqual(manifest["files"][name]["sha256"], sha(data))
        with tempfile.TemporaryDirectory(prefix="minikmb-cli-doc-test-") as directory:
            incomplete = Path(directory)
            (incomplete / "docs").mkdir()
            for name in ["CLI.md", "CLI.en.md"]:
                (incomplete / "docs" / name).write_text("fixture", encoding="utf-8")
            with self.assertRaises(FileNotFoundError):
                cli_documents(incomplete)

    def test_windows_paths_reject_traversal_aliases_and_reserved_devices(self):
        for name in ["../key", "/key", "C:/key", "a\\key", "a//key", "a/./key", "a/..", "a/CON.txt", "a/NUL", "a/key.", "a/key "]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                safe_name(name)
        self.assertEqual(safe_name("licenses/crates/demo-1.2/LICENSE-MIT"), "licenses/crates/demo-1.2/LICENSE-MIT")

    def test_normal_build_closure_excludes_dev_and_unreachable_packages(self):
        edge = lambda name, kind: {"pkg": name, "dep_kinds": [{"kind": kind}]}
        metadata = {"resolve": {"root": "app", "nodes": [
            {"id": "app", "deps": [edge("normal", None), edge("build", "build"), edge("dev", "dev")]},
            {"id": "normal", "deps": [edge("shared", None)]}, {"id": "build", "deps": [edge("shared", None)]},
            {"id": "shared", "deps": []}, {"id": "dev", "deps": []}, {"id": "unreachable", "deps": []}]}}
        self.assertEqual(dependency_closure(metadata), {"app", "normal", "build", "shared"})

    def test_crate_archive_paths_are_validated_without_extracting_payloads(self):
        data = archive_bytes({"demo-1/LICENSE": b"fixture", "demo-1/src/lib.rs": b"DO_NOT_COPY_SOURCE_INTO_NOTICES"})
        self.assertEqual(crate_members(data, "demo-1", notices_only=True), {"LICENSE": b"fixture"})
        for name in ["demo-1/../key", "other/LICENSE", "demo-1/C:/key"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                crate_members(archive_bytes({name: b"x"}), "demo-1")

    def test_pe_direct_delay_imports_and_truncated_addresses(self):
        for delay in [False, True]:
            result = pe_imports(pe_fixture(delay))
            self.assertEqual(result["machine"], "x86_64")
            self.assertEqual(result["subsystem"], 2)
            self.assertEqual(result["delay" if delay else "direct"], ["kernel32.dll"])
        for data in [b"", b"MZ", pe_fixture()[:250], pe_fixture()[:0x210]]:
            with self.assertRaises(ValueError):
                pe_imports(data)
        invalid = bytearray(pe_fixture())
        struct.pack_into("<I", invalid, 0x20c, 0xffffeeee)
        with self.assertRaises(ValueError):
            pe_imports(invalid)

    def test_zip_is_deterministic_and_hashes_every_listed_file(self):
        payload = {"minikmb-native.exe": b"generated fixture", "README.txt": b"fixture"}
        a, b = io.BytesIO(), io.BytesIO()
        write_zip(a, payload, {"channel": "preview"})
        write_zip(b, payload, {"channel": "preview"})
        self.assertEqual(a.getvalue(), b.getvalue())
        result = verify_zip(io.BytesIO(a.getvalue()))
        self.assertEqual(result["files"]["README.txt"]["sha256"], sha(b"fixture"))
        with self.assertRaises(ValueError):
            write_zip(io.BytesIO(), {"api-keys.json": b"secret"}, {})
        with self.assertRaises(ValueError):
            write_zip(io.BytesIO(), {"licenses/demo/LICENSE": b"a", "licenses/demo/license": b"b"}, {})

    def test_zip_corruption_extra_files_duplicate_paths_and_symlinks_are_rejected(self):
        data = io.BytesIO()
        write_zip(data, {"README.txt": b"expected"}, {})
        with zipfile.ZipFile(io.BytesIO(data.getvalue())) as original:
            members = {name: original.read(name) for name in original.namelist()}
        for mutate in [lambda m: m.update({"README.txt": b"corrupt"}),
                       lambda m: m.update({"api-keys.json": b"not allowed"}),
                       lambda m: m.pop("README.txt")]:
            candidate = dict(members)
            mutate(candidate)
            output = io.BytesIO()
            with zipfile.ZipFile(output, "w") as archive:
                for name, content in candidate.items():
                    archive.writestr(name, content)
            with self.assertRaises(ValueError):
                verify_zip(io.BytesIO(output.getvalue()))
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            for name, content in members.items():
                entry = zipfile.ZipInfo(name)
                entry.external_attr = (stat.S_IFLNK | 0o777) << 16
                archive.writestr(entry, content)
        with self.assertRaises(ValueError):
            verify_zip(io.BytesIO(output.getvalue()))

    def fixture(self, root, license_text="MIT"):
        (root / "licenses").mkdir()
        (root / "licenses/upstream.json").write_text('{"files":[]}', encoding="utf-8")
        (root / "assets/fonts").mkdir(parents=True)
        (root / "assets/manifest.json").write_text("{}", encoding="utf-8")
        source = root / "registry/src/index/demo-1.0.0"
        source.mkdir(parents=True)
        package = {"id": "demo", "name": "demo", "version": "1.0.0", "source": "registry+fixture", "license": license_text,
                   "manifest_path": str(source / "Cargo.toml"), "repository": "https://example.invalid/fixture", "authors": []}
        contents = {"LICENSE.txt": b"generated license fixture", "src/lib.rs": b"generated source fixture"}
        blob = archive_bytes({f"demo-1.0.0/{name}": value for name, value in contents.items()})
        cache = root / "registry/cache/index"
        cache.mkdir(parents=True)
        (cache / "demo-1.0.0.crate").write_bytes(blob)
        for name, value in contents.items():
            (source / name).parent.mkdir(parents=True, exist_ok=True)
            (source / name).write_bytes(value)
        (root / "Cargo.lock").write_text(f'[[package]]\nname="demo"\nversion="1.0.0"\nsource="registry+fixture"\nchecksum="{sha(blob)}"\n', encoding="utf-8")
        sysroot = root / "rust"
        (sysroot / "share/doc/rust/licenses").mkdir(parents=True)
        for name in ["MIT.txt", "Apache-2.0.txt"]:
            (sysroot / "share/doc/rust/licenses" / name).write_text("fixture", encoding="utf-8")
        (sysroot / "share/doc/rust/COPYRIGHT-library.html").write_text("fixture", encoding="utf-8")
        metadata = {"packages": [{"id":"app","name":"fixture-app","version":"0","source":None},package], "resolve": {"root": "app", "nodes": [
            {"id": "app", "deps": [{"pkg": "demo", "dep_kinds": [{"kind": None}]}]}, {"id": "demo", "deps": []}]}}
        return metadata, sysroot, source

    def test_locked_notice_collection_does_not_include_device_configuration_or_source_payload(self):
        with tempfile.TemporaryDirectory(prefix="minikmb-package-test-") as temp:
            root = Path(temp)
            metadata, sysroot, _ = self.fixture(root)
            (root / "api-keys.json").write_bytes(b"PRIVATE_KEY_NEVER_PACKAGE")
            (root / "operation.jsonl").write_bytes(b"PRIVATE_LOG_NEVER_PACKAGE")
            files, inventory = collect_notices(root, metadata, sysroot)
            self.assertEqual(len(inventory["packages"]), 1)
            self.assertTrue(any(name.endswith("LICENSE.txt") for name in files))
            self.assertFalse(any(name.endswith("lib.rs") for name in files))
            self.assertFalse(any(b"NEVER_PACKAGE" in data for data in files.values()))
            cache = root / "registry/cache/index/demo-1.0.0.crate"
            cache.write_bytes(cache.read_bytes() + b"changed")
            with self.assertRaisesRegex(ValueError, "checksum"):
                collect_notices(root, metadata, sysroot)

    def test_mpl_source_is_bundled_exactly_and_local_edits_are_not_hidden(self):
        with tempfile.TemporaryDirectory(prefix="minikmb-package-test-") as temp:
            root = Path(temp)
            metadata, sysroot, source = self.fixture(root, "MPL-2.0")
            files, inventory = collect_notices(root, metadata, sysroot)
            path = inventory["packages"][0]["bundled_source"]
            self.assertEqual(sha(files[path]), inventory["packages"][0]["source_checksum"])
            (source / "src/lib.rs").write_bytes(b"local edit")
            with self.assertRaisesRegex(ValueError, "locally changed"):
                collect_notices(root, metadata, sysroot)


if __name__ == "__main__":
    unittest.main()
