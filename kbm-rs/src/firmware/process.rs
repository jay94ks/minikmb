//! A bounded child process runner. Only SHA-pinned bundled tools are executable.
//! Read/preflight can stop; accepted writes finish verification before returning.
use super::backend::{Control, Notify, Progress};
use crate::model::AppError;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Manifest {
    tools: Vec<Tool>,
}
#[derive(Deserialize)]
struct Tool {
    id: String,
    file: String,
    sha256: String,
}
#[derive(Clone)]
pub struct Tools {
    pub directory: PathBuf,
}
pub struct Output {
    pub text: String,
    pub last: Option<Value>,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Read,
    Stm32Write,
    SpiWrite,
}
enum Line {
    Text(String),
    Eof,
    Error,
}

fn error(code: &'static str, message: &str) -> AppError {
    AppError::new(code, message)
}
impl Tools {
    pub fn bundled() -> Self {
        let directory = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("tools")))
            .unwrap_or_default();
        Self { directory }
    }
    fn verified(&self, id: &str) -> Result<(PathBuf, File), AppError> {
        let manifest: Manifest = serde_json::from_str(include_str!(
            "../../firmware-tools/tools.lock.json"
        ))
        .map_err(|_| {
            error(
                "TOOL_MANIFEST_INVALID",
                "동봉 도구 목록이 올바르지 않습니다.",
            )
        })?;
        let tool = manifest
            .tools
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| error("TOOL_NOT_FOUND", "동봉 플래싱 도구가 없습니다."))?;
        let path = self.directory.join(&tool.file);
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // Keep the executable immutable through process creation.
        }
        let mut file = options.open(&path).map_err(|_| {
            error(
                "TOOL_NOT_FOUND",
                "동봉 플래싱 도구를 찾을 수 없습니다. ZIP 전체를 새 폴더에 풀어주세요.",
            )
        })?;
        if file
            .metadata()
            .map_err(|_| error("TOOL_IO_FAILED", "도구 파일을 확인하지 못했습니다."))?
            .len()
            > 100 * 1024 * 1024
        {
            return Err(error(
                "TOOL_HASH_MISMATCH",
                "동봉 도구 크기가 올바르지 않습니다.",
            ));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|_| error("TOOL_IO_FAILED", "도구 파일을 읽지 못했습니다."))?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if format!("{:x}", hash.finalize()) != tool.sha256 {
            return Err(error(
                "TOOL_HASH_MISMATCH",
                "동봉 도구의 SHA256이 다릅니다. 실행하지 않았습니다.",
            ));
        }
        Ok((path, file))
    }

    pub fn run(
        &self,
        id: &str,
        args: &[OsString],
        mode: Mode,
        control: &Control,
        notify: &Notify,
    ) -> Result<Output, AppError> {
        control.check()?;
        let (tool, _guard) = self.verified(id)?;
        let work = tempfile::tempdir()
            .map_err(|_| error("TOOL_IO_FAILED", "도구 작업 폴더를 만들지 못했습니다."))?;
        // The explicit preset defeats an inherited default preset. Environment
        // variables and the command working directory are also isolated.
        let mut preset = None;
        if id == "swd" {
            let mut nonce = [0u8; 8];
            getrandom::fill(&mut nonce)
                .map_err(|_| error("RANDOM_FAILED", "도구 설정을 격리하지 못했습니다."))?;
            let name = format!(
                "minikmb_{}",
                nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            std::fs::write(work.path().join(".probe-rs.toml"),format!("[presets.{name}]\nallow-erase-all = false\ncycle-power = false\nchip-erase = false\ndry-run = false\n"))
                .map_err(|_|error("TOOL_IO_FAILED","도구 설정을 준비하지 못했습니다."))?;
            preset = Some(name);
        }
        let mut command = Command::new(&tool);
        command
            .args(args)
            .current_dir(work.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(preset) = &preset {
            command.arg("--preset").arg(preset);
        }
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("PROBE_RS_") {
                command.env_remove(name);
            }
        }
        for name in [
            "HOME",
            "USERPROFILE",
            "XDG_CONFIG_HOME",
            "APPDATA",
            "LOCALAPPDATA",
        ] {
            command.env(name, work.path());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut committed = mode == Mode::Stm32Write;
        if committed {
            (control.commit)()?;
            notify(Progress {
                phase: "STM32 기록·검증",
                done: 0,
                total: 0,
            });
        }
        let mut child = command.spawn().map_err(|_| {
            error(
                "TOOL_START_FAILED",
                "동봉 플래싱 도구를 실행하지 못했습니다.",
            )
        })?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut stdin = child.stdin.take();
        let (tx, rx) = mpsc::sync_channel(128);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let read = (&mut reader).take(1024 * 1024 + 1).read_line(&mut line);
                match read {
                    Ok(0) => break,
                    Ok(_) if line.len() <= 1024 * 1024 => {
                        let progress = line.starts_with("{\"event\":\"progress\"");
                        if progress {
                            let _ = tx.try_send(Line::Text(line));
                        } else if tx.send(Line::Text(line)).is_err() {
                            return;
                        }
                    }
                    _ => {
                        let _ = tx.send(Line::Error);
                        break;
                    }
                }
            }
            let _ = tx.send(Line::Eof);
        });
        let errors = thread::spawn(move || {
            let mut input = stderr;
            let mut kept = Vec::new();
            let mut bytes = [0; 4096];
            loop {
                match input.read(&mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let retain = n.min((64 * 1024usize).saturating_sub(kept.len()));
                        kept.extend_from_slice(&bytes[..retain]);
                    }
                }
            }
            kept
        });
        let started = Instant::now();
        let mut text = String::new();
        let mut last = None;
        let mut failure = None;
        let mut ended = false;
        loop {
            if !committed
                && (control.cancel.is_cancelled() || started.elapsed() > Duration::from_secs(90))
            {
                if let Some(input) = stdin.as_mut() {
                    let _ = input.write_all(b"N");
                }
                let _ = child.kill();
                failure = Some(if control.cancel.is_cancelled() {
                    error("OPERATION_CANCELLED", "펌웨어 작업을 취소했습니다.")
                } else {
                    error("TOOL_TIMEOUT", "기록 전 도구 응답 시간이 초과됐습니다.")
                });
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Line::Text(line)) => {
                    if text.len() + line.len() <= 1024 * 1024 {
                        text.push_str(&line);
                    }
                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                        let event = value["event"].as_str().unwrap_or_default();
                        match event {
                            "ready_to_write" if mode == Mode::SpiWrite => {
                                let accepted = (control.commit)();
                                let byte = if accepted.is_ok() { b"Y" } else { b"N" };
                                if let Some(input) = stdin.as_mut() {
                                    if input.write_all(byte).and_then(|_| input.flush()).is_err() {
                                        failure = Some(error(
                                            "TOOL_IO_FAILED",
                                            "플래시 도구 승인 전달에 실패했습니다.",
                                        ));
                                    }
                                }
                                if let Err(e) = accepted {
                                    failure = Some(e);
                                } else {
                                    committed = true;
                                    notify(Progress {
                                        phase: "SPI 기록·검증",
                                        done: 0,
                                        total: 0,
                                    });
                                }
                            }
                            "progress" => notify(Progress {
                                phase: match value["stage"].as_u64() {
                                    Some(0) => "SPI 읽기",
                                    Some(1) => "SPI 기록",
                                    Some(2) => "SPI 지우기",
                                    _ => "SPI 검증",
                                },
                                done: value["done"].as_u64().unwrap_or(0),
                                total: value["total"].as_u64().unwrap_or(0),
                            }),
                            "reading_original" => notify(Progress {
                                phase: "기존 플래시 읽기",
                                done: 0,
                                total: 0,
                            }),
                            "reading_verification" => notify(Progress {
                                phase: "전체 플래시 비교 검증",
                                done: 0,
                                total: 0,
                            }),
                            "error" => {
                                failure = Some(error(
                                    if value["code"] == "IMAGE_TOO_LARGE" {
                                        "FLASH_TOO_SMALL"
                                    } else {
                                        "FLASH_TOOL_FAILED"
                                    },
                                    "플래시 기록 도구가 작업을 거부했습니다.",
                                ))
                            }
                            _ => {}
                        }
                        last = Some(value);
                    }
                }
                Ok(Line::Error) => {
                    failure = Some(error(
                        "TOOL_RESPONSE_INVALID",
                        "플래싱 도구 응답을 확인하지 못했습니다.",
                    ));
                    if !committed {
                        let _ = child.kill();
                    }
                }
                Ok(Line::Eof) | Err(mpsc::RecvTimeoutError::Disconnected) => ended = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if ended
                && child
                    .try_wait()
                    .map_err(|_| error("TOOL_WAIT_FAILED", "도구 종료를 확인하지 못했습니다."))?
                    .is_some()
            {
                break;
            }
            if ended {
                thread::sleep(Duration::from_millis(20));
            }
        }
        drop(stdin);
        let status = child
            .wait()
            .map_err(|_| error("TOOL_WAIT_FAILED", "도구 종료를 확인하지 못했습니다."))?;
        let _ = reader.join();
        let _ = errors.join();
        if let Some(mut e) = failure {
            e.uncertain |= committed;
            return Err(e);
        }
        if !status.success() {
            return Err(if committed {
                AppError::uncertain(
                    "FLASH_TOOL_FAILED",
                    "플래시 기록/검증을 확인하지 못했습니다. 자동으로 재시도하지 않습니다.",
                )
            } else {
                error("TOOL_FAILED", "프로브/Writer 접속을 확인하지 못했습니다.")
            });
        }
        Ok(Output { text, last })
    }
}

pub fn staged_binary(work: &Path, bytes: &[u8]) -> Result<PathBuf, AppError> {
    let path = work.join("firmware.bin");
    std::fs::write(&path, bytes).map_err(|_| {
        error(
            "FIRMWARE_IO_FAILED",
            "작업용 펌웨어 파일을 만들지 못했습니다.",
        )
    })?;
    Ok(path)
}
