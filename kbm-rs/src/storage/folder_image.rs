//! Immutable FAT32 snapshot in a new anonymous regular file. Original files are
//! only opened for reading. No public API path is passed to this builder.
use super::{format::Guard, import::file_error};
use crate::model::AppError;
use serde::Serialize;
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
const CLUSTER: u64 = 4096;
const RESERVED: u32 = 32;
const MAX_ENTRIES: usize = 10000;
const MAX_METADATA: usize = 8 * 1024 * 1024;
const MAX_SECTORS: u64 = 67108864;
#[cfg(test)]
mod tests;

fn invalid() -> AppError {
    AppError::new("FOLDER_SNAPSHOT_FAILED", "폴더를 읽을 수 없거나 파일/경로가 변경됐습니다. 링크·특수 파일과 잘못된 FAT 이름은 공유하지 않습니다.")
}
fn limit() -> AppError {
    AppError::new("FOLDER_LIMIT", "폴더 공유는10,000개 항목·깊이32·메타데이터8MiB·이미지32GiB 이내이며 각 파일은4GiB 미만이어야 합니다.")
}
#[derive(Clone, PartialEq, Eq)]
struct Identity {
    size: u64,
    modified: Option<std::time::SystemTime>,
    created: Option<std::time::SystemTime>,
    directory: bool,
    #[cfg(windows)]
    id: (u32, u32, u32),
    #[cfg(windows)]
    changed: i64,
    #[cfg(unix)]
    id: (u64, u64),
    #[cfg(unix)]
    changed: (i64, i64),
}
impl Identity {
    fn same_object(&self, other: &Self) -> bool {
        self.id == other.id && self.created == other.created && self.directory == other.directory
    }
}
fn reparse(m: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        m.file_type().is_symlink()
    }
}
fn identity(file: &File) -> Result<Identity, AppError> {
    let m = file.metadata().map_err(|_| invalid())?;
    if reparse(&m) || (!m.is_file() && !m.is_dir()) {
        return Err(invalid());
    }
    #[cfg(windows)]
    let (id, changed) = {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| invalid())?;
        use windows::Win32::Storage::FileSystem::{
            FileBasicInfo, GetFileInformationByHandleEx, FILE_BASIC_INFO,
        };
        let mut basic = FILE_BASIC_INFO::default();
        unsafe {
            GetFileInformationByHandleEx(
                HANDLE(file.as_raw_handle()),
                FileBasicInfo,
                &mut basic as *mut _ as *mut _,
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        }
        .map_err(|_| invalid())?;
        (
            (
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
            ),
            basic.ChangeTime,
        )
    };
    #[cfg(unix)]
    let (id, changed) = {
        use std::os::unix::fs::MetadataExt;
        ((m.dev(), m.ino()), (m.ctime(), m.ctime_nsec()))
    };
    Ok(Identity {
        size: m.len(),
        modified: m.modified().ok(),
        created: m.created().ok(),
        directory: m.is_dir(),
        id,
        changed,
    })
}
fn final_path(file: &File, path: &Path) -> Result<PathBuf, AppError> {
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetFinalPathNameByHandleW, GETFINALPATHNAMEBYHANDLE_FLAGS},
        };
        let _ = path;
        let mut b = vec![0; 32768];
        let len = unsafe {
            GetFinalPathNameByHandleW(
                HANDLE(file.as_raw_handle()),
                &mut b,
                GETFINALPATHNAMEBYHANDLE_FLAGS(0),
            )
        } as usize;
        if len == 0 || len >= b.len() {
            return Err(invalid());
        }
        Ok(std::ffi::OsString::from_wide(&b[..len]).into())
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        path.canonicalize().map_err(|_| invalid())
    }
}
fn open(path: &Path, root: &Path) -> Result<(File, Identity), AppError> {
    let m = fs::symlink_metadata(path).map_err(|_| invalid())?;
    if reparse(&m) || (!m.is_file() && !m.is_dir()) {
        return Err(invalid());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x02200000);
    } // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
    let file = options.open(path).map_err(|_| invalid())?;
    if !final_path(&file, path)?.starts_with(root) {
        return Err(invalid());
    }
    let info = identity(&file)?;
    Ok((file, info))
}
#[derive(Clone)]
pub struct Root {
    path: PathBuf,
    identity: Identity,
}
impl Root {
    pub fn approve(path: PathBuf) -> Result<Self, AppError> {
        if !path.is_absolute() || reparse(&fs::symlink_metadata(&path).map_err(|_| invalid())?) {
            return Err(invalid());
        }
        let path = path.canonicalize().map_err(|_| invalid())?;
        let (_, info) = open(&path, &path)?;
        if !info.directory {
            return Err(invalid());
        }
        Ok(Self {
            path,
            identity: info,
        })
    }
    pub fn display_name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "선택한 폴더".into())
    }
}
struct Entry {
    name: String,
    path: PathBuf,
    identity: Identity,
    parent: Option<usize>,
    children: Vec<usize>,
    first: u32,
    clusters: u32,
}
#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct Summary {
    pub files: u32,
    pub bytes: u64,
    pub sectors: u32,
    pub populated_sectors: u32,
}
pub struct Snapshot {
    pub file: File,
    pub summary: Summary,
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.encode_utf16().count() <= 255
        && !name.chars().any(|c| c < ' ' || "\\/:*?\"<>|".contains(c))
        && !name.ends_with(['.', ' '])
}
struct Tree {
    entries: Vec<Entry>,
    metadata: usize,
    files: u32,
    bytes: u64,
}
impl Tree {
    fn walk(
        &mut self,
        root: &Path,
        path: PathBuf,
        name: String,
        parent: Option<usize>,
        depth: usize,
        guard: &Guard,
    ) -> Result<usize, AppError> {
        guard()?;
        if depth > 32 || self.entries.len() >= MAX_ENTRIES {
            return Err(limit());
        }
        if !valid_name(&name) {
            return Err(invalid());
        }
        let (handle, info) = open(&path, root)?;
        if !info.directory && info.size > u64::from(u32::MAX) {
            return Err(limit());
        }
        let index = self.entries.len();
        self.entries.push(Entry {
            name,
            path: path.clone(),
            identity: info.clone(),
            parent,
            children: vec![],
            first: 0,
            clusters: 0,
        });
        let mut clusters = info.size.div_ceil(CLUSTER) as u32;
        if info.directory {
            let mut children = vec![];
            for entry in fs::read_dir(&path).map_err(|_| invalid())? {
                guard()?;
                let entry = entry.map_err(|_| invalid())?;
                let m = fs::symlink_metadata(entry.path()).map_err(|_| invalid())?;
                if reparse(&m) || (!m.is_file() && !m.is_dir()) {
                    continue;
                }
                if children.len() + self.entries.len() >= MAX_ENTRIES {
                    return Err(limit());
                }
                children.push((
                    entry.file_name().into_string().map_err(|_| invalid())?,
                    entry.path(),
                ));
            }
            children.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            let mut names = std::collections::HashSet::new();
            let mut slots = if parent.is_some() { 3 } else { 2 };
            for (name, child) in children {
                if !names.insert(name.to_uppercase()) {
                    return Err(invalid());
                }
                slots += name.encode_utf16().count().div_ceil(13) + 1;
                let child = self.walk(root, child, name, Some(index), depth + 1, guard)?;
                self.entries[index].children.push(child);
            }
            clusters = (slots as u64 * 32).div_ceil(CLUSTER).max(1) as u32;
            self.metadata += clusters as usize * CLUSTER as usize;
            if self.metadata > MAX_METADATA {
                return Err(limit());
            }
        } else {
            self.files += 1;
            self.bytes += info.size;
            if self.bytes > MAX_SECTORS * 512 {
                return Err(limit());
            }
        }
        if identity(&handle)? != info {
            return Err(invalid());
        }
        self.entries[index].clusters = clusters;
        Ok(index)
    }
}
fn put16(b: &mut [u8], at: usize, n: u16) {
    b[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn alias(index: usize, name: &str, directory: bool) -> [u8; 11] {
    let mut n = index;
    let mut b = *b"F0000000   ";
    for at in (1..8).rev() {
        b[at] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"[n % 36];
        n /= 36;
    }
    if !directory {
        if let Some((_, extension)) = name.rsplit_once('.') {
            for (at, c) in extension
                .to_uppercase()
                .bytes()
                .filter(u8::is_ascii_alphanumeric)
                .take(3)
                .enumerate()
            {
                b[8 + at] = c;
            }
        }
    }
    b
}
fn slots(e: &Entry, alias: [u8; 11]) -> Vec<[u8; 32]> {
    let name: Vec<_> = e.name.encode_utf16().collect();
    let count = name.len().div_ceil(13);
    let checksum = alias
        .iter()
        .fold(0u8, |s, b| s.rotate_right(1).wrapping_add(*b));
    let mut entries = Vec::with_capacity(count + 1);
    for ordinal in (1..=count).rev() {
        let mut b = [0; 32];
        b[0] = ordinal as u8 | if ordinal == count { 0x40 } else { 0 };
        b[11] = 15;
        b[13] = checksum;
        for (at, pos) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
            .iter()
            .enumerate()
        {
            let n = (ordinal - 1) * 13 + at;
            put16(
                &mut b,
                *pos,
                if n == name.len() {
                    0
                } else {
                    name.get(n).copied().unwrap_or(0xffff)
                },
            );
        }
        entries.push(b);
    }
    let mut b = [0; 32];
    b[..11].copy_from_slice(&alias);
    b[11] = if e.identity.directory { 0x11 } else { 0x21 };
    for at in [16, 18, 24] {
        put16(&mut b, at, 0x21);
    }
    put16(&mut b, 20, (e.first >> 16) as u16);
    put16(&mut b, 26, e.first as u16);
    if !e.identity.directory {
        put32(&mut b, 28, e.identity.size as u32);
    }
    entries.push(b);
    entries
}
fn write_at(file: &mut File, at: u64, b: &[u8]) -> Result<(), AppError> {
    file.seek(SeekFrom::Start(at)).map_err(|_| file_error())?;
    file.write_all(b).map_err(|_| file_error())
}
pub fn build(root: Root, guard: Guard) -> Result<Snapshot, AppError> {
    guard()?;
    let (root_handle, root_info) = open(&root.path, &root.path)?;
    if !root_info.same_object(&root.identity) {
        return Err(invalid());
    }
    let mut tree = Tree {
        entries: vec![],
        metadata: 0,
        files: 0,
        bytes: 0,
    };
    tree.walk(
        &root.path,
        root.path.clone(),
        "ROOT".into(),
        None,
        0,
        &guard,
    )?;
    let mut next = 2u64;
    for e in &mut tree.entries {
        if e.clusters != 0 {
            e.first = next as u32;
            next += u64::from(e.clusters);
        }
    }
    let used = next - 2;
    let clusters = 65536u64.max(used + 1024);
    let fat_sectors = ((clusters + 2) * 4).div_ceil(512) as u32;
    let data_start = RESERVED + 2 * fat_sectors;
    let sectors = u64::from(data_start) + clusters * 8;
    if sectors > MAX_SECTORS {
        return Err(limit());
    }
    let mut file = tempfile::tempfile().map_err(|_| file_error())?;
    file.set_len(sectors * 512).map_err(|_| file_error())?;
    let mut boot = [0; 512];
    boot[..3].copy_from_slice(&[0xeb, 0x58, 0x90]);
    boot[3..11].copy_from_slice(b"KBM FAT ");
    put16(&mut boot, 11, 512);
    boot[13] = 8;
    put16(&mut boot, 14, RESERVED as u16);
    boot[16] = 2;
    boot[21] = 0xf8;
    put16(&mut boot, 24, 63);
    put16(&mut boot, 26, 255);
    put32(&mut boot, 32, sectors as u32);
    put32(&mut boot, 36, fat_sectors);
    put32(&mut boot, 44, 2);
    put16(&mut boot, 48, 1);
    put16(&mut boot, 50, 6);
    boot[64] = 0x80;
    boot[66] = 0x29;
    let mut serial = [0; 4];
    getrandom::fill(&mut serial).map_err(|_| file_error())?;
    boot[67..71].copy_from_slice(&serial);
    boot[71..82].copy_from_slice(b"KBM SHARE  ");
    boot[82..90].copy_from_slice(b"FAT32   ");
    put16(&mut boot, 510, 0xaa55);
    let mut fsinfo = [0; 512];
    put32(&mut fsinfo, 0, 0x41615252);
    put32(&mut fsinfo, 484, 0x61417272);
    put32(&mut fsinfo, 488, (clusters - used) as u32);
    put32(&mut fsinfo, 492, (used + 2) as u32);
    put32(&mut fsinfo, 508, 0xaa550000);
    for lba in [0u32, 6] {
        write_at(&mut file, u64::from(lba) * 512, &boot)?;
    }
    for lba in [1u32, 7] {
        write_at(&mut file, u64::from(lba) * 512, &fsinfo)?;
    }
    let mut fat = vec![0; fat_sectors as usize * 512];
    put32(&mut fat, 0, 0x0ffffff8);
    put32(&mut fat, 4, 0xffffffff);
    for e in &tree.entries {
        for n in 0..e.clusters {
            if n % 4096 == 0 {
                guard()?;
            }
            put32(
                &mut fat,
                (e.first + n) as usize * 4,
                if n + 1 == e.clusters {
                    0x0fffffff
                } else {
                    e.first + n + 1
                },
            );
        }
    }
    for lba in [RESERVED, RESERVED + fat_sectors] {
        guard()?;
        write_at(&mut file, u64::from(lba) * 512, &fat)?;
    }
    let mut alias_index = 0;
    for e in &tree.entries {
        guard()?;
        let (mut source, info) = open(&e.path, &root.path)?;
        if info != e.identity {
            return Err(invalid());
        }
        let at = u64::from(data_start) * 512 + u64::from(e.first.saturating_sub(2)) * CLUSTER;
        if e.identity.directory {
            let mut content = vec![0; e.clusters as usize * CLUSTER as usize];
            let mut pos = 0;
            if let Some(parent) = e.parent {
                for (name, first) in [
                    (b".          ", e.first),
                    (
                        b"..         ",
                        if parent == 0 {
                            0
                        } else {
                            tree.entries[parent].first
                        },
                    ),
                ] {
                    content[pos..pos + 11].copy_from_slice(name);
                    content[pos + 11] = 0x10;
                    put16(&mut content, pos + 20, (first >> 16) as u16);
                    put16(&mut content, pos + 26, first as u16);
                    pos += 32;
                }
            } else {
                content[..11].copy_from_slice(b"KBM SHARE  ");
                content[11] = 8;
                pos = 32;
            }
            let reserved: std::collections::HashMap<_, _> = e
                .children
                .iter()
                .map(|&i| (tree.entries[i].name.to_ascii_uppercase(), i))
                .collect();
            for &child in &e.children {
                let short = loop {
                    alias_index += 1;
                    let a = alias(
                        alias_index,
                        &tree.entries[child].name,
                        tree.entries[child].identity.directory,
                    );
                    let stem = String::from_utf8_lossy(&a[..8]);
                    let extension = String::from_utf8_lossy(&a[8..]);
                    let key = if extension.trim_end().is_empty() {
                        stem.to_string()
                    } else {
                        format!("{}.{}", stem, extension.trim_end())
                    };
                    if reserved.get(&key).is_none_or(|&i| i == child) {
                        break a;
                    }
                };
                for slot in slots(&tree.entries[child], short) {
                    content[pos..pos + 32].copy_from_slice(&slot);
                    pos += 32;
                }
            }
            write_at(&mut file, at, &content)?;
        } else if e.identity.size != 0 {
            file.seek(SeekFrom::Start(at)).map_err(|_| file_error())?;
            let mut remaining = e.identity.size;
            let mut buffer = [0; 65536];
            while remaining != 0 {
                guard()?;
                let count = remaining.min(buffer.len() as u64) as usize;
                source
                    .read_exact(&mut buffer[..count])
                    .map_err(|_| invalid())?;
                file.write_all(&buffer[..count]).map_err(|_| file_error())?;
                remaining -= count as u64;
            }
        }
        if identity(&source)? != info {
            return Err(invalid());
        }
    }
    // A directory/file changed after its bytes were copied still invalidates
    // this generation. Later host edits cannot affect the finished temp image.
    for e in &tree.entries {
        guard()?;
        if open(&e.path, &root.path)?.1 != e.identity {
            return Err(invalid());
        }
    }
    if identity(&root_handle)? != root_info {
        return Err(invalid());
    }
    file.seek(SeekFrom::Start(0)).map_err(|_| file_error())?;
    Ok(Snapshot {
        file,
        summary: Summary {
            files: tree.files,
            bytes: tree.bytes,
            sectors: sectors as u32,
            populated_sectors: data_start + used as u32 * 8,
        },
    })
}
