use eframe::egui::{self, RichText};
use minikmb_native::{
    active::Binding,
    input_sessions::Principal,
    model::AppError,
    service::AppService,
    theme::{self, color},
    tty,
};
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct View {
    status: Option<tty::Status>,
    bytes: VecDeque<u8>,
    stream: Option<String>,
    cursor: Option<u64>,
    skipped: u64,
    error: String,
    sent: bool,
}
pub struct Terminal {
    pub open: bool,
    binding: Option<Binding>,
    principal: Principal,
    view: Arc<Mutex<View>>,
    busy: Arc<AtomicBool>,
    text: String,
    hex: bool,
    crlf: bool,
    poll: Instant,
    preview: bool,
}
impl Default for Terminal {
    fn default() -> Self {
        Self {
            open: false,
            binding: None,
            principal: Principal {
                key_id: "ui".into(),
                client_id: "ui:tty".into(),
                cancelled: CancellationToken::new(),
                ended: CancellationToken::new(),
            },
            view: Default::default(),
            busy: Default::default(),
            text: String::new(),
            hex: false,
            crlf: false,
            poll: Instant::now(),
            preview: false,
        }
    }
}
impl Terminal {
    pub fn preview() -> Self {
        let mut terminal = Self::default();
        let uid = minikmb_native::model::Uid::from_bytes(&[0; 12]).unwrap();
        terminal.toggle(Binding {
            uid: uid.clone(),
            generation: 0,
        });
        terminal.preview = true;
        let mut view = terminal.view.lock();
        view.bytes="UI 미리보기 · 실제 장치 데이터가 아닙니다.\r\n타깃 CDC 출력은 이 영역에 표시됩니다.\r\n".as_bytes().iter().copied().collect();
        view.status = Some(tty::Status {
            uid,
            generation: 0,
            receiving: false,
            stream_id: "preview".into(),
            bytes_received: 0,
            bytes_retained: 0,
            bytes_overwritten: 0,
            bytes_accepted: 0,
            capacity: 65536,
            error: String::new(),
            line: tty::LineStatus {
                target_open: false,
                baud: 115200,
                data_bits: 8,
                ..Default::default()
            },
        });
        drop(view);
        terminal
    }
    pub fn toggle(&mut self, b: Binding) {
        if self.open {
            self.close();
            return;
        }
        self.principal.cancelled = CancellationToken::new();
        self.principal.ended = CancellationToken::new();
        self.binding = Some(b);
        self.view = Default::default();
        self.busy = Default::default();
        self.open = true;
        self.poll = Instant::now() - Duration::from_secs(1);
    }
    fn close(&mut self) {
        self.open = false;
        self.principal.ended.cancel();
        self.binding = None;
    }
    fn command(
        &mut self,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        name: &'static str,
        args: serde_json::Value,
    ) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(b) = self.binding.clone() else {
            self.busy.store(false, Ordering::Release);
            return;
        };
        let app = app.clone();
        let p = self.principal.clone();
        let busy = self.busy.clone();
        let view = self.view.clone();
        runtime.spawn(async move {
            let result = app.execute_tty(b, &p, name, args).await;
            let mut s = view.lock();
            match result {
                Ok(v) => {
                    s.error.clear();
                    s.sent =
                        name == "tty_write" && v.get("complete") == Some(&serde_json::json!(true));
                    if v.get("complete") == Some(&serde_json::json!(false)) {
                        s.error = format!(
                            "{} / {} 바이트 수락. {}",
                            v["accepted"],
                            v["total"],
                            v["error"]["message"].as_str().unwrap_or("송신 중단")
                        );
                    }
                }
                Err(e) => s.error = e.message,
            }
            busy.store(false, Ordering::Release);
        });
    }
    fn refresh(&mut self, app: &Arc<AppService>, runtime: &tokio::runtime::Handle) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(b) = self.binding.clone() else {
            self.busy.store(false, Ordering::Release);
            return;
        };
        let app = app.clone();
        let p = self.principal.clone();
        let view = self.view.clone();
        let busy = self.busy.clone();
        runtime.spawn(async move {
            let result = async {
                if p.ended.is_cancelled() {
                    return Ok(());
                }
                let status = app.tty.status(b.clone(), p.clone()).await?;
                {
                    let mut s = view.lock();
                    if s.stream.as_ref().is_some_and(|id| id != &status.stream_id) {
                        s.bytes.clear();
                        s.cursor = None;
                        s.stream = None;
                        s.skipped = 0;
                    }
                    s.status = Some(status.clone());
                }
                if status.stream_id.ends_with("-0") {
                    return Ok(());
                }
                for _ in 0..16 {
                    let (stream, cursor) = {
                        let s = view.lock();
                        (s.stream.clone(), s.cursor)
                    };
                    let read = app.tty.read(&b, stream.as_deref(), cursor, 4096)?;
                    let done = read.cursor == read.end_cursor;
                    let mut s = view.lock();
                    s.stream = Some(read.stream_id);
                    s.cursor = Some(read.cursor);
                    s.skipped += read.skipped;
                    for byte in read.bytes {
                        if s.bytes.len() == tty::CAPACITY {
                            s.bytes.pop_front();
                        }
                        s.bytes.push_back(byte);
                    }
                    if done {
                        break;
                    }
                }
                Ok::<_, AppError>(())
            }
            .await;
            if let Err(e) = result {
                view.lock().error = e.message;
            }
            busy.store(false, Ordering::Release);
        });
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        active: &minikmb_native::active::ActiveState,
    ) {
        if !self.open {
            return;
        }
        {
            let mut s = self.view.lock();
            if s.sent {
                self.text.clear();
                s.sent = false;
            }
        }
        if !self.preview
            && !self.binding.as_ref().is_some_and(|b| {
                Some(&b.uid) == active.uid.as_ref()
                    && b.generation == active.generation
                    && !active.switching
            })
        {
            self.close();
            return;
        }
        if !self.preview && self.poll.elapsed() > Duration::from_millis(250) {
            self.poll = Instant::now();
            self.refresh(app, runtime);
        }
        let mut opened = true;
        theme::tool_window(ctx,if self.preview {"시리얼 중계 · UI 미리보기"} else {"시리얼 중계"},"target-tty",[760.0,450.0]).open(&mut opened).show(ctx,|ui|{
            let busy=self.preview || self.busy.load(Ordering::Acquire);
            ui.horizontal_wrapped(|ui|{
                if ui.add_enabled(!busy,egui::Button::new("수신 시작")).clicked(){self.command(app,runtime,"tty_open",serde_json::json!({}));}
                if ui.add_enabled(!busy,egui::Button::new("수신 중지")).clicked(){self.command(app,runtime,"tty_close",serde_json::json!({}));}
                ui.checkbox(&mut self.hex,"HEX");ui.checkbox(&mut self.crlf,"송신 끝 CRLF");
                if ui.button("표시 지우기").clicked(){self.view.lock().bytes.clear();}
            });
            let s=self.view.lock();
            if let Some(status)=&s.status {
                theme::status(ui,if status.receiving{theme::Tone::Success}else{theme::Tone::Neutral},if status.receiving{"수신 중"}else{"수신 중지"});
                ui.label(RichText::new(format!("타깃 CDC {} · {} baud · {} data bits · parity {} / stop {} · TX/RX 큐 {}/{}",if status.line.target_open{"열림"}else{"닫힘"},status.line.baud,status.line.data_bits,status.line.parity,status.line.stop_bits,status.line.tx_queued,status.line.rx_queued)).small().color(color::MUTED));
                ui.label(RichText::new(format!("수신 {} B · 수락 {} B · 타깃 TX/RX 폐기 {}/{} · 읽기 누락 {} B",status.bytes_received,status.bytes_accepted,status.line.tx_discarded,status.line.rx_discarded,s.skipped)).small().color(color::MUTED));
                if !status.error.is_empty(){ui.colored_label(color::ERROR,&status.error);}
            }
            if !s.error.is_empty(){ui.colored_label(color::ERROR,&s.error);}
            let bytes:Vec<_>=s.bytes.iter().copied().collect();drop(s);
            let display=if self.hex{bytes.iter().map(|b|format!("{b:02X}")).collect::<Vec<_>>().join(" ")}else{String::from_utf8_lossy(&bytes).into_owned()};
            egui::Frame::new().fill(color::INPUT).inner_margin(10).show(ui,|ui|{egui::ScrollArea::both().id_salt("tty-output").auto_shrink([false,false]).max_height(240.0).min_scrolled_height(180.0).stick_to_bottom(true).show(ui,|ui|{ui.label(RichText::new(display).monospace());});});
            ui.horizontal(|ui|{
                let edit=ui.add_enabled(!busy,egui::TextEdit::singleline(&mut self.text).char_limit(196608).hint_text(if self.hex{"송신 HEX (공백 허용)"}else{"송신 텍스트"}).desired_width(ui.available_width()-80.0));
                let enter=edit.lost_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter));
                let send=ui.add_enabled(!busy&&!self.text.is_empty(),egui::Button::new("전송")).clicked();
                if !busy&&!self.text.is_empty()&&(send||enter){if enter{edit.request_focus();}match encode(&self.text,self.hex,self.crlf){Ok(bytes)=>{self.command(app,runtime,"tty_write",serde_json::json!({"bytes":bytes}));},Err(e)=>self.view.lock().error=e.message}}
            });
        });
        if !opened {
            self.close();
        }
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        self.close();
    }
}
fn encode(text: &str, hex: bool, crlf: bool) -> Result<Vec<u8>, AppError> {
    let mut bytes = if hex {
        let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if cleaned.is_empty() || !cleaned.len().is_multiple_of(2) || !cleaned.is_ascii() {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "올바른 HEX 바이트를 입력하세요.",
            ));
        }
        (0..cleaned.len())
            .step_by(2)
            .map(|at| {
                u8::from_str_radix(&cleaned[at..at + 2], 16).map_err(|_| {
                    AppError::new("INVALID_ARGUMENTS", "올바른 HEX 바이트를 입력하세요.")
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let mut b = text.as_bytes().to_vec();
        if crlf {
            b.extend_from_slice(b"\r\n");
        }
        b
    };
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "시리얼 송신은 1~65536바이트입니다.",
        ));
    }
    bytes.shrink_to_fit();
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_and_hex_match_v1_without_lossy_hex() {
        assert_eq!(encode("한글", false, true).unwrap(), "한글\r\n".as_bytes());
        assert_eq!(encode("00 aF\nff", true, true).unwrap(), vec![0, 175, 255]);
        assert!(encode("0", true, false).is_err());
        assert!(encode("가", true, false).is_err());
        assert!(encode(&"F".repeat(131074), true, false).is_err());
    }
}
