//! Only newly allocated app-owned files are passed to mkfs/fsck. No public path argument.
use super::*;
use futures_util::future::BoxFuture;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub type Guard = Arc<dyn Fn() -> Result<(), AppError> + Send + Sync>;
#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub format: u8,
    pub bytes: u64,
    pub existing: bool,
}
impl Request {
    pub fn validate(self) -> Result<Self, AppError> {
        let minimum = match self.format {
            1 => 64 * 1048576,
            2 => 1048576,
            3 => 8 * 1048576,
            _ => return Err(invalid()),
        };
        if self.bytes < minimum
            || self.bytes > 32768 * 1048576
            || !self.bytes.is_multiple_of(512)
            || (!self.existing
                && (self.bytes < 64 * 1048576 || !self.bytes.is_multiple_of(1048576)))
        {
            return Err(invalid());
        }
        Ok(self)
    }
}
fn invalid() -> AppError {
    AppError::new("INVALID_ARGUMENTS","새 이미지는64..32768MiB입니다. 포맷할 기존 이미지는512B 단위로 FAT32≥64MiB, exFAT≥1MiB, EXT4≥8MiB여야 합니다.")
}
fn io() -> AppError {
    AppError::new(
        "IMAGE_IO_FAILED",
        "포맷용 임시 이미지 파일을 준비하지 못했습니다.",
    )
}
pub trait Engine: Send + Sync {
    fn generate(
        &self,
        request: Request,
        guard: Guard,
    ) -> BoxFuture<'static, Result<Artifact, AppError>>;
}
pub struct Native;
impl Engine for Native {
    fn generate(
        &self,
        request: Request,
        guard: Guard,
    ) -> BoxFuture<'static, Result<Artifact, AppError>> {
        Box::pin(async move {
            let request = request.validate()?;
            guard()?;
            tokio::task::spawn_blocking(move || generate(request, guard))
                .await
                .map_err(|_| io())?
        })
    }
}
struct Directory {
    dir: Option<tempfile::TempDir>,
    canonical: PathBuf,
    parent: PathBuf,
    process_closed: bool,
}
impl Directory {
    fn new() -> Result<Self, AppError> {
        let parent = std::env::temp_dir().canonicalize().map_err(|_| io())?;
        let dir = tempfile::Builder::new()
            .prefix("minikmb-format-")
            .tempdir_in(&parent)
            .map_err(|_| io())?;
        let canonical = match dir.path().canonicalize() {
            Ok(p) => p,
            Err(_) => {
                let _ = dir.keep();
                return Err(io());
            }
        };
        let mut this = Self {
            dir: Some(dir),
            canonical,
            parent,
            process_closed: true,
        };
        if !this.safe() {
            this.preserve();
            return Err(io());
        }
        Ok(this)
    }
    fn safe(&self) -> bool {
        self.process_closed
            && self.canonical.parent() == Some(self.parent.as_path())
            && self
                .canonical
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("minikmb-format-"))
            && self
                .dir
                .as_ref()
                .is_some_and(|d| d.path().canonicalize().is_ok_and(|p| p == self.canonical))
    }
    fn path(&self, name: &str) -> PathBuf {
        self.canonical.join(name)
    }
    fn preserve(&mut self) {
        if let Some(dir) = self.dir.take() {
            let _ = dir.keep();
        }
    }
    fn close(mut self) -> Result<(), AppError> {
        if !self.safe() {
            self.preserve();
            return Err(cleanup_error());
        }
        self.dir
            .take()
            .unwrap()
            .close()
            .map_err(|_| cleanup_error())
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if self.dir.is_some() {
            if self.safe() {
                if let Some(dir) = self.dir.take() {
                    let _ = dir.close();
                }
            } else {
                self.preserve();
            }
        }
    }
}
fn cleanup_error() -> AppError {
    AppError::new(
        "FORMAT_CLEANUP_FAILED",
        "앱 소유 임시 이미지의 정리를 완료하지 못했습니다.",
    )
}
pub struct Artifact {
    file: Option<File>,
    directory: Option<Directory>,
}
impl Artifact {
    pub fn take_file(&mut self) -> File {
        self.file.take().expect("owned image file")
    }
    pub fn close(mut self) -> Result<(), AppError> {
        self.file.take();
        if let Some(directory) = self.directory.take() {
            directory.close()?;
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn mock(request: Request) -> Result<Self, AppError> {
        use std::io::{Seek, SeekFrom, Write};
        let request = request.validate()?;
        let mut file = tempfile::tempfile().map_err(|_| io())?;
        file.set_len(request.bytes).map_err(|_| io())?;
        let (at, b): (_, &[u8]) = match request.format {
            1 => (82, b"FAT32   "),
            2 => (3, b"EXFAT   "),
            _ => (1080, &[0x53, 0xef]),
        };
        file.seek(SeekFrom::Start(at)).map_err(|_| io())?;
        file.write_all(b).map_err(|_| io())?;
        Ok(Self {
            file: Some(file),
            directory: None,
        })
    }
}
fn linux_path(path: &Path) -> Result<String, AppError> {
    let text = path
        .to_str()
        .ok_or_else(io)?
        .strip_prefix(r"\\?\")
        .unwrap_or(path.to_str().ok_or_else(io)?);
    let b = text.as_bytes();
    if b.len() < 4 || !b[0].is_ascii_alphabetic() || b[1] != b':' || ![b'/', b'\\'].contains(&b[2])
    {
        return Err(AppError::new(
            "FORMAT_TEMP_UNSUPPORTED",
            "WSL에서 접근 가능한 로컬 드라이브 임시 폴더가 필요합니다.",
        ));
    }
    Ok(format!(
        "/mnt/{}/{}",
        (b[0] as char).to_ascii_lowercase(),
        text[3..].replace('\\', "/")
    ))
}
const SUPERVISOR: &str = include_str!("format_supervisor.py");
#[derive(Deserialize)]
struct Done {
    token: String,
    closed: bool,
    ok: bool,
    reason: String,
}
fn generate(request: Request, guard: Guard) -> Result<Artifact, AppError> {
    guard()?;
    let mut directory = Directory::new()?;
    let image = directory.path("disk.img");
    {
        let f = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&image)
            .map_err(|_| io())?;
        f.set_len(request.bytes).map_err(|_| io())?;
    }
    let linux = linux_path(&image)?;
    let lease = directory.path("lease");
    let done = directory.path("done");
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).map_err(|_| io())?;
    let token: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&lease, b"0").map_err(|_| io())?;
    let mut command = Command::new("wsl.exe");
    command
        .args([
            "--exec",
            "/usr/bin/python3",
            "-c",
            SUPERVISOR,
            &linux,
            &request.bytes.to_string(),
            &request.format.to_string(),
            &token,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    directory.process_closed = false;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            directory.process_closed = true;
            return Err(AppError::new(
                "FORMAT_TOOL_UNAVAILABLE",
                "WSL 이미지 생성 도구를 시작하지 못했습니다.",
            ));
        }
    };
    let deadline = Instant::now() + Duration::from_secs(255);
    let mut abort = None;
    let mut aborted_at = None;
    let mut beat = 0u64;
    let mut last_beat = Instant::now();
    loop {
        if abort.is_none() {
            if let Err(e) = guard() {
                abort = Some(e);
                aborted_at = Some(Instant::now());
                let _ = std::fs::write(&lease, b"cancel");
            }
        }
        if abort.is_none() && last_beat.elapsed() >= Duration::from_millis(250) {
            beat += 1;
            if std::fs::write(&lease, beat.to_string()).is_err() {
                abort = Some(io());
                aborted_at = Some(Instant::now());
            }
            last_beat = Instant::now();
        }
        let exited = child.try_wait().map_err(|_| io())?;
        if let Some(status) = exited {
            let receipt = std::fs::read(&done)
                .ok()
                .filter(|b| b.len() <= 2048)
                .and_then(|b| serde_json::from_slice::<Done>(&b).ok());
            let Some(receipt) = receipt.filter(|d| d.token == token && d.closed) else {
                directory.preserve();
                return Err(AppError::new(
                    "FORMAT_PROCESS_UNCONFIRMED",
                    "이미지 도구의 종료를 확인하지 못해 임시 폴더를 보존했습니다.",
                ));
            };
            directory.process_closed = true;
            if let Some(error) = abort {
                directory.close()?;
                return Err(error);
            }
            guard()?;
            if !status.success() || !receipt.ok {
                let reason = match receipt.reason.as_str() {
                    "mkfs" => "파일 시스템 생성 실패",
                    "fsck" => "파일 시스템 검사 실패",
                    "timeout" => "도구 시간 초과",
                    "missing_tool" => "WSL 파일 시스템 도구가 없습니다.",
                    _ => "도구 실행 중단",
                };
                directory.close()?;
                return Err(AppError::new("FORMAT_TOOL_FAILED", reason));
            }
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .open(&image)
                .map_err(|_| io())?;
            if file.metadata().map_err(|_| io())?.len() != request.bytes {
                return Err(io());
            }
            let mut bytes = [0; 2048];
            file.read_exact(&mut bytes).map_err(|_| io())?;
            let valid = match request.format {
                1 => &bytes[82..90] == b"FAT32   ",
                2 => &bytes[3..11] == b"EXFAT   ",
                3 => bytes[1080..1082] == [0x53, 0xef],
                _ => false,
            };
            if !valid {
                return Err(AppError::new(
                    "FORMAT_VERIFY_FAILED",
                    "생성한 이미지의 파일 시스템 서명이 다릅니다.",
                ));
            }
            return Ok(Artifact {
                file: Some(file),
                directory: Some(directory),
            });
        }
        if Instant::now() > deadline
            || aborted_at.is_some_and(|t| t.elapsed() > Duration::from_secs(7))
        {
            let _ = std::fs::write(&lease, b"cancel");
            let _ = child.kill();
            let _ = child.wait();
            directory.preserve();
            return Err(AppError::new(
                "FORMAT_PROCESS_UNCONFIRMED",
                "이미지 도구의 종료를 확인하지 못해 임시 폴더를 보존했습니다.",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
impl Work {
    pub(super) async fn format_image(&mut self, action: Action) -> Result<Value, AppError> {
        let (format, create_mib, id, expected_size, expected_card_uuid) = match action {
            Action::DiskCreate {
                format,
                size_mib,
                expected_card_uuid,
                ..
            } => (format, Some(size_mib), None, None, expected_card_uuid),
            Action::DiskFormat {
                id,
                format,
                expected_size,
                expected_card_uuid,
                ..
            } => (format, None, Some(id), expected_size, expected_card_uuid),
            _ => unreachable!(),
        };
        self.check()?;
        let card = self.media_card(expected_card_uuid.as_deref()).await?;
        let bytes = if let Some(mib) = create_mib {
            u64::from(mib) * 1048576
        } else {
            let snapshot = self.refresh().await?;
            let item = snapshot
                .images
                .iter()
                .find(|i| i.kind == "disk" && Some(i.id) == id)
                .ok_or_else(|| {
                    AppError::new("IMAGE_NOT_FOUND", "선택한 디스크 이미지가 없습니다.")
                })?;
            if expected_size.is_some_and(|size| size != item.size) {
                return Err(AppError::new(
                    "IMAGE_CHANGED",
                    "선택 이후 이미지 용량이 변경됐습니다.",
                ));
            }
            item.size
        };
        let request = Request {
            format,
            bytes,
            existing: id.is_some(),
        }
        .validate()?;
        match self.command(3, 0x318, vec![]).await {
            Err(e) if e.status == Some(17) => {}
            Err(e) => return Err(e),
            Ok(_) => {
                return Err(AppError::new(
                    "MEDIA_CONFLICT",
                    "모든 디스크 경로를 닫은 뒤 이미지를 생성/포맷하세요.",
                ))
            }
        };
        self.media_card(Some(&card.uuid)).await?;
        self.phase("임시 이미지 생성·파일 시스템 검사");
        let job = self.job.clone();
        let active = self.manager.active.clone();
        let wire = self.wire.clone();
        let guard: Guard = Arc::new(move || {
            if job.cancel.is_cancelled() {
                return Err(AppError::new(
                    "OPERATION_CANCELLED",
                    "이미지 생성을 취소했습니다.",
                ));
            }
            check_owner(&job.owner)?;
            active.validate(&job.binding)?;
            if !wire.live() {
                return Err(AppError::new(
                    "STALE_SESSION",
                    "이미지 준비 중 MCU 연결이 종료됐습니다.",
                ));
            }
            Ok(())
        });
        let mut artifact = self.manager.formatter.generate(request, guard).await?;
        let spec = import::Spec::Disk {
            size: bytes,
            format,
            replace_id: id,
            confirmed: true,
            expected_card_uuid: Some(card.uuid),
        };
        let result = self.import_image(spec, artifact.take_file()).await;
        let cleanup = artifact.close();
        if let Err(error) = cleanup {
            self.host_cleanup = Some(error.clone());
            return match result {
                Ok(_) => Err(error),
                Err(original) => Err(original),
            };
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_and_wsl_paths_cannot_select_a_host_device() {
        for format in 1..=3 {
            Request {
                format,
                bytes: 64 * 1048576,
                existing: false,
            }
            .validate()
            .unwrap();
        }
        Request {
            format: 2,
            bytes: 9 * 1048576 + 512,
            existing: true,
        }
        .validate()
        .unwrap();
        assert!(Request {
            format: 1,
            bytes: 9 * 1048576 + 512,
            existing: true
        }
        .validate()
        .is_err());
        for bytes in [0, 1048576, 67108865, 32768 * 1048576 + 512] {
            assert!(Request {
                format: 2,
                bytes,
                existing: false
            }
            .validate()
            .is_err());
        }
        assert_eq!(
            linux_path(Path::new(r"C:\private\a b\disk.img")).unwrap(),
            "/mnt/c/private/a b/disk.img"
        );
        assert_eq!(
            linux_path(Path::new(r"\\?\C:\private\한글\disk.img")).unwrap(),
            "/mnt/c/private/한글/disk.img"
        );
        for path in [
            r"\\server\share\disk.img",
            "/dev/sda",
            "disk.img",
            r"C:disk.img",
        ] {
            assert!(linux_path(Path::new(path)).is_err());
        }
    }
    #[tokio::test]
    #[ignore = "Explicit local WSL acceptance: only app-owned temporary regular image files, no MCU/SD or physical disks"]
    async fn owned_wsl_images_validate_signatures_exact_size_cancel_and_cleanup() {
        for (format, bytes, existing) in [
            (1, 67108864, false),
            (2, 67108864, false),
            (3, 67108864, false),
            (2, 9 * 1048576 + 512, true),
        ] {
            let artifact = Native
                .generate(
                    Request {
                        format,
                        bytes,
                        existing,
                    },
                    Arc::new(|| Ok(())),
                )
                .await
                .unwrap();
            let root = artifact.directory.as_ref().unwrap().canonical.clone();
            assert!(root.is_absolute());
            assert!(artifact.directory.as_ref().unwrap().safe());
            assert_eq!(
                artifact.file.as_ref().unwrap().metadata().unwrap().len(),
                bytes
            );
            artifact.close().unwrap();
            assert!(!root.exists());
        }
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = count.clone();
        let error = Native
            .generate(
                Request {
                    format: 3,
                    bytes: 67108864,
                    existing: false,
                },
                Arc::new(move || {
                    if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 3 {
                        Err(AppError::new("OPERATION_CANCELLED", "test cancellation"))
                    } else {
                        Ok(())
                    }
                }),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "OPERATION_CANCELLED");
    }
}
