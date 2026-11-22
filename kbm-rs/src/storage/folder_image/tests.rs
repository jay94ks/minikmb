use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
fn guard() -> Guard {
    Arc::new(|| Ok(()))
}
fn sector(file: &mut File, lba: u32) -> [u8; 512] {
    let mut b = [0; 512];
    file.seek(SeekFrom::Start(u64::from(lba) * 512)).unwrap();
    file.read_exact(&mut b).unwrap();
    b
}
fn u16at(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes(b[n..n + 2].try_into().unwrap())
}
fn u32at(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(b[n..n + 4].try_into().unwrap())
}
fn root_slots(file: &mut File, first: u32) -> Vec<[u8; 32]> {
    let mut result = vec![];
    for lba in first..first + 8 {
        for b in sector(file, lba).chunks_exact(32) {
            if b[0] == 0 {
                return result;
            }
            result.push(b.try_into().unwrap());
        }
    }
    result
}
fn long_names(slots: &[[u8; 32]]) -> Vec<(String, [u8; 32])> {
    let mut result = vec![];
    let mut long = std::collections::BTreeMap::new();
    let mut checksum = None;
    for b in slots {
        if b[11] == 15 {
            let text: Vec<_> = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
                .iter()
                .map(|n| u16at(b, *n))
                .collect();
            long.insert(b[0] & 31, text);
            checksum = Some(b[13]);
        } else if b[11] & 8 == 0 {
            let c = b[..11]
                .iter()
                .fold(0u8, |s, n| s.rotate_right(1).wrapping_add(*n));
            if let Some(expected) = checksum {
                assert_eq!(c, expected);
            }
            let units: Vec<_> = long
                .values()
                .flatten()
                .copied()
                .take_while(|n| *n != 0 && *n != 0xffff)
                .collect();
            result.push((String::from_utf16(&units).unwrap(), *b));
            long.clear();
            checksum = None;
        }
    }
    result
}
#[test]
fn fat_tables_vfat_unicode_nested_empty_and_multicluster_content_are_consistent() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("긴 이름과 이모지🚀.txt"), vec![0xa5; 6000]).unwrap();
    fs::write(dir.path().join("empty.bin"), []).unwrap();
    fs::create_dir(dir.path().join("packages")).unwrap();
    fs::write(dir.path().join("packages/tool.deb"), b"package payload").unwrap();
    let mut snap = build(Root::approve(dir.path().to_path_buf()).unwrap(), guard()).unwrap();
    assert_eq!(snap.summary.files, 3);
    assert_eq!(snap.summary.bytes, 6015);
    let boot = sector(&mut snap.file, 0);
    assert_eq!(&boot[82..90], b"FAT32   ");
    assert_eq!(u16at(&boot, 510), 0xaa55);
    assert_eq!(boot, sector(&mut snap.file, 6));
    assert_eq!(u16at(&boot, 11), 512);
    assert_eq!(boot[13], 8);
    let fat = u32at(&boot, 36);
    let data = 32 + 2 * fat;
    let info = sector(&mut snap.file, 1);
    assert_eq!(u32at(&info, 0), 0x41615252);
    assert_eq!(u32at(&info, 484), 0x61417272);
    for n in 0..fat {
        assert_eq!(
            sector(&mut snap.file, 32 + n),
            sector(&mut snap.file, 32 + fat + n)
        );
    }
    let names = long_names(&root_slots(&mut snap.file, data));
    assert_eq!(
        names.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        vec!["empty.bin", "packages", "긴 이름과 이모지🚀.txt"]
    );
    for (name, b) in names {
        assert_ne!(b[11] & 1, 0);
        let first = (u32::from(u16at(&b, 20)) << 16) | u32::from(u16at(&b, 26));
        match name.as_str() {
            "empty.bin" => {
                assert_eq!(first, 0);
                assert_eq!(u32at(&b, 28), 0);
            }
            "packages" => {
                let slots = root_slots(&mut snap.file, data + (first - 2) * 8);
                assert_eq!(&slots[0][..11], b".          ");
                assert_eq!(u16at(&slots[1], 26), 0);
                let child = long_names(&slots[2..]);
                assert_eq!(child[0].0, "tool.deb");
                let cluster = u32::from(u16at(&child[0].1, 26));
                assert_eq!(
                    &sector(&mut snap.file, data + (cluster - 2) * 8)[..15],
                    b"package payload"
                );
            }
            _ => {
                assert_eq!(u32at(&b, 28), 6000);
                let fat0 = sector(&mut snap.file, 32 + first / 128);
                assert_eq!(u32at(&fat0, (first % 128 * 4) as usize), first + 1);
                snap.file
                    .seek(SeekFrom::Start(
                        u64::from(data) * 512 + u64::from(first - 2) * 4096,
                    ))
                    .unwrap();
                let mut bytes = vec![0; 8192];
                snap.file.read_exact(&mut bytes).unwrap();
                assert!(bytes[..6000].iter().all(|n| *n == 0xa5));
                assert!(bytes[6000..].iter().all(|n| *n == 0));
            }
        }
    }
    assert!(sector(&mut snap.file, snap.summary.sectors - 1)
        .iter()
        .all(|b| *b == 0));
    assert_eq!(
        fs::read(dir.path().join("packages/tool.deb")).unwrap(),
        b"package payload"
    );
}
#[test]
fn finished_snapshot_is_immutable_and_manual_refresh_reads_new_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("key.txt");
    fs::write(&path, b"old key").unwrap();
    let root = Root::approve(dir.path().to_path_buf()).unwrap();
    let mut old = build(root.clone(), guard()).unwrap();
    fs::write(&path, b"new key").unwrap();
    fs::write(dir.path().join("new.txt"), b"another").unwrap();
    let mut fresh = build(root, guard()).unwrap();
    assert_eq!(fresh.summary.files, 2);
    let boot = sector(&mut old.file, 0);
    let data = 32 + 2 * u32at(&boot, 36);
    let first = long_names(&root_slots(&mut old.file, data))[0].1;
    assert_eq!(
        &sector(&mut old.file, data + (u32::from(u16at(&first, 26)) - 2) * 8)[..7],
        b"old key"
    );
    let entries = long_names(&root_slots(&mut fresh.file, data));
    let key = entries.iter().find(|(n, _)| n == "key.txt").unwrap().1;
    assert_eq!(
        &sector(&mut fresh.file, data + (u32::from(u16at(&key, 26)) - 2) * 8)[..7],
        b"new key"
    );
}
#[test]
fn invalid_names_size_and_cancel_fail_before_a_usable_image() {
    assert!(!valid_name("trailing."));
    assert!(!valid_name("bad:name"));
    assert!(!valid_name(&"a".repeat(256)));
    assert!(valid_name("한글😀.rpm"));
    let dir = tempfile::tempdir().unwrap();
    let root = Root::approve(dir.path().to_path_buf()).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let n = count.clone();
    let error = build(
        root,
        Arc::new(move || {
            if n.fetch_add(1, Ordering::Relaxed) > 2 {
                Err(AppError::new("OPERATION_CANCELLED", "test"))
            } else {
                Ok(())
            }
        }),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "OPERATION_CANCELLED");
    File::create(dir.path().join("too-big.bin"))
        .unwrap()
        .set_len(1u64 << 32)
        .unwrap();
    assert_eq!(
        build(Root::approve(dir.path().to_path_buf()).unwrap(), guard())
            .err()
            .unwrap()
            .code,
        "FOLDER_LIMIT"
    );
}
#[cfg(windows)]
#[test]
fn directory_junction_is_not_followed_or_approved_as_a_root() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private.txt"), b"outside").unwrap();
    let link = dir.path().join("junction");
    let quote = |p: &Path| format!("'{}'", p.to_str().unwrap().replace('\'', "''"));
    let script = format!(
        "New-Item -ItemType Junction -Path {} -Value {} -ErrorAction Stop | Out-Null",
        quote(&link),
        quote(outside.path())
    );
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(Root::approve(link.clone()).is_err());
    let snap = build(Root::approve(dir.path().to_path_buf()).unwrap(), guard()).unwrap();
    assert_eq!(snap.summary.files, 0);
    fs::remove_dir(link).unwrap();
    assert_eq!(
        fs::read(outside.path().join("private.txt")).unwrap(),
        b"outside"
    );
}

#[cfg(windows)]
#[test]
#[ignore = "Explicit read-only WSL fsck validation of a new app-owned temporary FAT32 image; no MCU/SD"]
fn generated_fat32_passes_readonly_wsl_fsck() {
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("폴더")).unwrap();
    fs::write(
        source.path().join("폴더/전달할 패키지.deb"),
        vec![0x5a; 9000],
    )
    .unwrap();
    fs::write(source.path().join("empty.bin"), []).unwrap();
    fs::write(
        source.path().join("fixture-key.txt"),
        b"app-owned fixture key",
    )
    .unwrap();
    let mut snapshot = build(Root::approve(source.path().to_path_buf()).unwrap(), guard()).unwrap();
    let mut staged = tempfile::Builder::new()
        .prefix("minikmb-folder-fsck-")
        .suffix(".img")
        .tempfile()
        .unwrap();
    staged
        .as_file_mut()
        .set_len(u64::from(snapshot.summary.sectors) * 512)
        .unwrap();
    std::io::copy(
        &mut Read::take(
            &mut snapshot.file,
            u64::from(snapshot.summary.populated_sectors) * 512,
        ),
        staged.as_file_mut(),
    )
    .unwrap();
    staged.as_file_mut().flush().unwrap();
    let canonical = staged.path().canonicalize().unwrap();
    let p = canonical
        .to_str()
        .unwrap()
        .strip_prefix(r"\\?\")
        .unwrap()
        .replace('\\', "/");
    assert_eq!(p.as_bytes()[1], b':');
    let linux = format!("/mnt/{}/{}", p[..1].to_ascii_lowercase(), &p[3..]);
    let output = std::process::Command::new("wsl.exe")
        .args(["--exec", "/usr/sbin/fsck.fat", "-n", &linux])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let read = std::process::Command::new("wsl.exe")
        .args([
            "--exec",
            "/usr/bin/mtype",
            "-i",
            &linux,
            "::/fixture-key.txt",
        ])
        .output()
        .unwrap();
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    assert_eq!(read.stdout, b"app-owned fixture key");
}

#[test]
fn synthetic_short_aliases_do_not_shadow_another_long_filename() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("F0000002.TXT"), b"real").unwrap();
    fs::write(dir.path().join("z.txt"), b"other").unwrap();
    let mut snap = build(Root::approve(dir.path().to_path_buf()).unwrap(), guard()).unwrap();
    let boot = sector(&mut snap.file, 0);
    let data = 32 + 2 * u32at(&boot, 36);
    let entries = long_names(&root_slots(&mut snap.file, data));
    assert_eq!(
        &entries.iter().find(|(n, _)| n == "z.txt").unwrap().1[..11],
        b"F0000003TXT"
    );
}

#[test]
fn replacing_the_approved_root_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let selected = dir.path().join("selected");
    let moved = dir.path().join("original");
    fs::create_dir(&selected).unwrap();
    let root = Root::approve(selected.clone()).unwrap();
    assert!(selected.starts_with(dir.path()) && moved.starts_with(dir.path()));
    fs::rename(&selected, &moved).unwrap();
    fs::create_dir(selected).unwrap();
    assert_eq!(
        build(root, guard()).err().unwrap().code,
        "FOLDER_SNAPSHOT_FAILED"
    );
}
